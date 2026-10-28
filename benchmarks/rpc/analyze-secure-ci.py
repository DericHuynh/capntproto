"""Analyze saved CI droplet artifacts; never launches benchmark workloads."""
import argparse, collections, importlib.util, json, math, os, re, statistics, subprocess, sys, time
from pathlib import Path
from allocation_comparison import compare as compare_allocations, markdown as allocation_markdown
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--native-only', action='store_true', help='Validate a latency-only campaign; do not require or claim profiling evidence.')
parser.add_argument('run_id')
parser.add_argument('directory', type=Path)
parser.add_argument('--saved-root', type=Path, help='Analyze already extracted evidence inside the measurement job.')
parser.add_argument('--measurement-outcome', choices=['success', 'failure'], help='Explicit measurement step outcome when analyzing saved CI evidence.')
args = parser.parse_args()
run_id, directory = args.run_id, args.directory
assert run_id.isdecimal()
directory.mkdir(parents=True, exist_ok=True)
def command(*args):
    return subprocess.check_output(args, text=True)
if args.saved_root:
    root = args.saved_root
    run = dict(headSha=os.environ['GITHUB_SHA'], url=f"https://github.com/{os.environ['GITHUB_REPOSITORY']}/actions/runs/{run_id}",
               status='analysis-before-upload', conclusion=None)
    provenance = dict(run=run, measurement_outcome=args.measurement_outcome, source='saved droplet archive; final job status must be checked separately')
    summaries = [root/'native/summary.json']
else:
    deadline = time.monotonic() + 50 * 60
    while True:
        run = json.loads(command('gh', 'run', 'view', run_id, '--json', 'status,conclusion,headSha,url'))
        if run['status'] == 'completed':
            break
        assert time.monotonic() < deadline, 'measurement did not finish before analysis deadline'
        time.sleep(30)
    print('MEASUREMENT_RUN', json.dumps(run), flush=True)
    artifacts = json.loads(command('gh', 'api', f"repos/{os.environ['GH_REPO']}/actions/runs/{run_id}/artifacts"))
    provenance = dict(run=run, artifacts=[a for a in artifacts['artifacts'] if a['name']=='dedicated-benchmark-report'])
    command('gh', 'run', 'download', run_id, '--name', 'dedicated-benchmark-report', '--dir', str(directory/'raw'))
    summaries = list((directory/'raw').glob('**/native/summary.json'))
    profiles_only = list((directory/'raw').glob('**/profiles/scope.json'))
    assert len(summaries)==1 or (not summaries and len(profiles_only)==1), 'ambiguous campaign root'
    root = (summaries or profiles_only)[0].parent.parent
(directory/'provenance.json').write_text(json.dumps(provenance, indent=2)+'\n')
# A missing diagnostic scope must not prevent salvaging completed native data.
profile_scope=root/'profiles/scope.json'
profiles=json.loads(profile_scope.read_text()) if not (root/'native/summary.json').exists() and profile_scope.exists() and not args.native_only else {}
if profiles.get('allocation_only') or profiles.get('instruction_only'):
    assert profiles['diagnostic']
    report=dict(run=run, measurement_outcome=args.measurement_outcome, allocation_only=bool(profiles.get('allocation_only')), instruction_only=bool(profiles.get('instruction_only')),
        host=json.loads((root.parent/'host.json').read_text()), hardware=(root/'cpu.txt').read_text(),
        binaries=profiles['binaries'], native_trials=0, aggregate={}, latency=[])
else:
    native = json.loads(summaries[0].read_text())
    scope = json.loads((root/'native/scope.json').read_text())
    assert not scope['diagnostic'] and not native['diagnostic']
    assert scope['repetitions']==5 and scope['iterations']==1000 and len(native['rows'])==48
    if scope.get('comparison_version') == 1:
        spec = importlib.util.spec_from_file_location('comparison_runner', Path(__file__).with_name('secure-runner.py'))
        comparison_runner = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(comparison_runner)
        assert {'grpc', 'websocket'} <= scope['binaries'].keys(), 'missing comparison executables'
        expected = set()
        for row in native['rows']:
            labels = {label for label in scope['binaries'] if comparison_runner.supported(label, row['transport'], row['case'])}
            assert row['variants'].keys() == labels, 'missing or unsupported protocol comparison'
            expected.update((row['transport'], row['case']['name'], label, rep) for label in labels for rep in range(5))
        actual = [(trial['transport'], trial['case'], trial['client'], trial['repetition']) for trial in scope['trials']]
        assert len(actual) == len(expected) and set(actual) == expected, 'incomplete protocol comparison repetitions'
    cells=collections.defaultdict(list)
    minima=collections.defaultdict(list)
    host_samples=[]
    quotas=set()
    def cpu_rows(value):
        return {line.split()[0]: list(map(int,line.split()[1:9])) for line in (value or '').splitlines() if line.startswith('cpu')}
    def named_counts(value):
        return {parts[0]: int(parts[1]) for line in (value or '').splitlines() if len(parts:=line.split())==2 and parts[1].isdigit()}
    for trial in scope['trials']:
        assert not trial['mixed']
        data=json.loads((root/'native'/trial['path']/'trial.json').read_text())
        assert data['iterations']==len(data['latency_ns'])==1000
        assert data['security']['tls']=='1.3' and data['security']['authentication']=='server-certificate'
        assert all(type(n) is int and n>0 for n in data['latency_ns'])
        cells[trial['transport'],trial['case'],trial['client']].append(statistics.mean(data['latency_ns']))
        minima[trial['transport'],trial['case'],trial['client']].append(min(data['latency_ns']))
        state=json.loads((root/'native'/trial['path']/'host.json').read_text())
        before,after=state['before'],state['after']
        quotas.add(str(before.get('/sys/fs/cgroup/cpu.max')))
        first,last=cpu_rows(before.get('/proc/stat')),cpu_rows(after.get('/proc/stat'))
        steals={}
        for cpu in first.keys() & last.keys():
            delta=[b-a for a,b in zip(first[cpu],last[cpu])]
            if sum(delta)>0:steals[cpu]=100*delta[7]/sum(delta)
        first,last=named_counts(before.get('/sys/fs/cgroup/cpu.stat')),named_counts(after.get('/sys/fs/cgroup/cpu.stat'))
        counters_available=all(key in counts for counts in (first,last) for key in ('nr_throttled','throttled_usec'))
        host_samples.append(dict(transport=trial['transport'],case=trial['case'],variant=trial['client'],path=trial['path'],steal_percent=steals,cpu_stat_available=counters_available,throttled_usec=(last.get('throttled_usec',0)-first.get('throttled_usec',0)) if counters_available else None,nr_throttled=(last.get('nr_throttled',0)-first.get('nr_throttled',0)) if counters_available else None))
    for row in native['rows']:
        for label,cell in row['variants'].items():
            values=cells[row['transport'],row['case']['name'],label]
            assert len(values)==5
            assert cell['min_ns']==min(minima[row['transport'],row['case']['name'],label])
            assert math.isclose(statistics.mean(values),cell['mean_ns'],rel_tol=1e-12)
            assert math.isclose(statistics.stdev(values),cell['repetition_mean_stddev_ns'],rel_tol=1e-12)
    aggregate={}
    for transport in ['tls','quic']:
        rows=[r for r in native['rows'] if r['transport']==transport]
        aggregate[transport]={}
        candidate = 'legacy' if 'legacy-baseline' in scope['binaries'] else 'portable'
        references = ['legacy-baseline', 'portable', 'cpp'] if candidate == 'legacy' else ['portable-baseline', 'legacy', 'cpp']
        references += ['grpc', 'websocket']
        for reference in references:
            pairs=[(r['variants'][candidate]['min_ns'],r['variants'][reference]['min_ns']) for r in rows if reference in r['variants']]
            if pairs:
                aggregate[transport][reference]=dict(estimator="minimum",cases=len(pairs),lower_minima=sum(a<b for a,b in pairs),
                    geometric_mean_latency_change_percent=100*(math.exp(statistics.mean(math.log(a/b) for a,b in pairs))-1))
    host=json.loads((root.parent/'host.json').read_text())
    report=dict(run=run,measurement_outcome=args.measurement_outcome,host=host,hardware=(root/'cpu.txt').read_text(),
        binaries=scope['binaries'],native_trials=len(scope['trials']),aggregate=aggregate,latency=native['rows'])
    report['host_counters']=dict(quotas=sorted(quotas),trials=len(host_samples),maximum_all_cpu_steal_percent=max((s['steal_percent'].get('cpu',0) for s in host_samples),default=0),maximum_single_cpu_steal_percent=max((v for s in host_samples for k,v in s['steal_percent'].items() if k!='cpu'),default=0),trials_with_cpu_stat=sum(s['cpu_stat_available'] for s in host_samples),throttled_trials=sum((s['nr_throttled'] or 0)>0 for s in host_samples) if any(s['cpu_stat_available'] for s in host_samples) else None,total_throttled_usec=sum(s['throttled_usec'] or 0 for s in host_samples) if any(s['cpu_stat_available'] for s in host_samples) else None)
    (directory/'host-counters.json').write_text(json.dumps(host_samples,indent=2)+'\n')
    # Retain the independently validated latency phase even if a later profile
    # is truncated. This checkpoint does not certify the diagnostic campaign.
    (directory/'native-summary.json').write_text(json.dumps(dict(report, diagnostics_complete=False),indent=2)+'\n')
    print('NATIVE_AGGREGATE',json.dumps(aggregate),flush=True)
if args.native_only:
    report.update(native_only=True, diagnostics_requested=False, diagnostics_complete=False,
                  profile_trials=0, profiles=[], traces=[])
    (directory/'summary.json').write_text(json.dumps(report,indent=2)+'\n')
    print('SUMMARY_SAVED',directory/'summary.json','native only; diagnostics not requested',flush=True)
    raise SystemExit(0)
profiles=json.loads((root/'profiles/scope.json').read_text())
raw=[];arena=[];allocation_sites=[]
for trial in profiles['trials']:
    for peer in ['client', *[f"service-{service['hop']}" for service in trial['services']]]:
        path=root/'profiles'/trial['path']
        if trial['mode']=='callgrind':
            events=values=None
            with (path/(peer+'.callgrind')).open() as file:
                for line in file:
                    if line.startswith('events:'):events=line.split()[1:]
                    if line.startswith('summary:'):values=list(map(int,line.split()[1:]))
                    if events is not None and values is not None:break
            assert events is not None and values is not None
            counts=dict(zip(events,values))
        else:
            data=json.loads((path/(peer+'.dhat.json')).read_text())
            counts={key:sum(p[field] for p in data['pps']) for key,field in [('blocks','tbk'),('bytes','tb')]}
            sites=[dict(blocks=p['tbk'],bytes=p['tb'],frames=[data['ftbl'][i] for i in p['fs']]) for p in data['pps'] if any('ReaderArena' in data['ftbl'][i] for i in p['fs'])]
            arena.append(dict(transport=trial['transport'],case=trial['case'],variant=trial['implementation'],iterations=trial['iterations'],peer=peer,sites=sites))
            allocation_sites.append(dict(transport=trial['transport'],case=trial['case'],variant=trial['implementation'],iterations=trial['iterations'],points=trial.get('points',[50,500]),peer=peer,source=path/(peer+'.dhat.json')))
        raw.append(dict(points=trial.get('points',[50,500]),mode=trial['mode'],transport=trial['transport'],case=trial['case'],variant=trial['implementation'],iterations=trial['iterations'],peer=peer,counts=counts))
index={(r['mode'],r['transport'],r['case'],r['variant'],r['iterations'],r['peer']):r for r in raw}
marginal=[]
totals=collections.defaultdict(collections.Counter)
for r in raw:
    lo,hi=r['points']
    if r['iterations']!=hi:continue
    short=index.get((r['mode'],r['transport'],r['case'],r['variant'],lo,r['peer']))
    if short is None: continue
    counts={k:(v-short['counts'].get(k,0))/(hi-lo) for k,v in r['counts'].items()}
    marginal.append({k:v for k,v in r.items() if k not in ['counts','iterations']}|dict(per_operation=counts))
    totals[r['mode'],r['transport'],r['case'],r['variant']].update(counts)
report['profiles']=[dict(mode=k[0],transport=k[1],case=k[2],variant=k[3],per_operation=v) for k,v in totals.items()]
report['profile_peers']=marginal
report['arena_counts']=[{k:v for k,v in r.items() if k!='sites'}|dict(blocks=sum(p['blocks'] for p in r['sites'])) for r in arena]
report['profile_trials']=len(profiles['trials'])
report['profile_failures']=profiles.get('failures', [])
report['unpaired_profile_peers']=[{k:r[k] for k in ['mode','transport','case','variant','iterations','peer']} for r in raw if any((r['mode'],r['transport'],r['case'],r['variant'],n,r['peer']) not in index for n in r['points'])]
if profiles.get('allocation_workloads'):
    comparison = compare_allocations(report, profiles)
    (directory/'allocation-comparison.json').write_text(json.dumps(comparison, indent=2)+'\n')
    (directory/'allocation-comparison.md').write_text(allocation_markdown(comparison))
    report['allocation_comparison'] = {k: v for k, v in comparison.items() if k != 'rows'}
traces=[]
for n in [10,100]:
    path=root/f'trace-{n}'
    if profiles.get('allocation_only') or (profiles.get('instruction_only') and report['native_trials']==0): continue
    trace_scope=json.loads((path/'scope.json').read_text())
    journals=json.loads((path/'ciphertext-counts.json').read_text())['journals']
    groups=collections.defaultdict(list)
    for trial in trace_scope['trials']:
        total=collections.Counter()
        for peer in ['client', *[f"service-{service['hop']}" for service in trial['services']]]:
            total.update(journals[trial['path']+'/'+peer+'.strace']['counts'])
        groups[trial['transport'],trial['case'],trial['client']].append(dict(total))
    for (transport,case,label),values in groups.items():
        assert len(values)==5
        keys=set().union(*(v.keys() for v in values))
        traces.append(dict(iterations=n,transport=transport,case=case,variant=label,
            counts={k:dict(mean=statistics.mean(v.get(k,0) for v in values),sd=statistics.stdev(v.get(k,0) for v in values)) for k in sorted(keys)}))
report['traces']=traces
(directory/'summary.json').write_text(json.dumps(report,indent=2)+'\n')
print('SUMMARY_SAVED', directory/'summary.json', 'profile_trials', report['profile_trials'], flush=True)
# Exclusive instruction attribution from saved profiles; simulated, not native CPU time.
pattern=re.compile(r'^\s*([\d,]+|\.)\s+([\d,]+|\.)\s+(.*)$')
num=lambda s:0 if s=='.' else int(s.replace(',',''))
def normalize(source):
    for prefix in ['/home/runner/work/capntproto/capntproto/target/legacy-baseline-source/', '/home/runner/work/capntproto/capntproto/target/portable-baseline-source/','/home/runner/work/capntproto/capntproto/']:
        source=source.removeprefix(prefix)
    return source
def category(source):
    if source.startswith('crates/capntproto-rpc/src/scoped') or source.startswith('crates/capntproto-rpc/src/portable'):return 'portable_hosted'
    if source.startswith('crates/capntproto-rpc-wire/'):return 'portable_wire'
    if source.startswith('crates/capntproto-rpc-engine/'):return 'portable_engine'
    if source.startswith('crates/capntproto-rpc/src/rpc'):return 'legacy_rpc'
    if source.startswith('crates/capntproto-rpc/'):return 'shared_rpc'
    if source.startswith('crates/capntproto-core/'):return 'serialization'
    if source.startswith('src/rpc/'):return 'transport_adapter'
    if '/malloc/' in source:return 'allocator'
    if 'ring-' in source or 'rustls-' in source:return 'tls_crypto'
    if 'quiche-' in source or 'boring-' in source:return 'quic_dependencies'
    if 'tokio-' in source or 'mio-' in source or 'futures-' in source:return 'async_dependencies'
    if 'memcpy' in source or 'memmove' in source or 'memset' in source:return 'memory_routines'
    if '/rustc/' in source:return 'rust_std'
    return 'other'
attribution=[]
for trial in profiles['trials']:
    if trial['mode']!='callgrind' or trial['implementation']=='cpp':continue
    for peer in ['client', *[f"service-{service['hop']}" for service in trial['services']]]:
        path=root/'profiles'/trial['path']/(peer+'.callgrind')
        annotated=command('callgrind_annotate','--inclusive=no','--show=Ir,Bcm','--show-percs=no','--threshold=100','--auto=no',str(path))
        files=collections.defaultdict(collections.Counter);functions=collections.defaultdict(collections.Counter);categories=collections.defaultdict(collections.Counter);parsed=collections.Counter();total=None
        for line in annotated.splitlines():
            match=pattern.match(line)
            if not match:continue
            ir,bcm,label=match.groups();cost=dict(Ir=num(ir),Bcm=num(bcm))
            if label=='PROGRAM TOTALS':total=cost;continue
            parsed.update(cost)
            source=normalize(label.split(' [',1)[0].split(':',1)[0])
            files[source].update(cost);functions[normalize(label.split(' [',1)[0])].update(cost);categories[category(source)].update(cost)
        assert total and parsed['Ir']>0 and all(parsed[k]<=v for k,v in total.items()),(path,total,parsed)
        # Annotated functions can omit a few termination/threshold instructions.
        # Preserve the exact raw summary as the authoritative total and expose
        # every residual; never silently drop it from component attribution.
        residual={k:v-parsed[k] for k,v in total.items()}
        if any(residual.values()):
            files['[unattributed Callgrind summary]'].update(residual)
            categories['unattributed'].update(residual)
        attribution.append(dict(points=trial.get('points',[50,500]),iterations=trial['iterations'],transport=trial['transport'],case=trial['case'],variant=trial['implementation'],peer=peer,files=dict(files),functions=dict(functions),categories=dict(categories)))
indexed={(r['iterations'],r['transport'],r['case'],r['variant'],r['peer']):r for r in attribution}
costs=[]
for key,long in indexed.items():
    lo,hi=long['points']
    if key[0]!=hi:continue
    short=indexed.get((lo,*key[1:]))
    if short is None: continue
    row={k:long[k] for k in ['transport','case','variant','peer']}
    for kind in ['files','functions','categories']:
        keys=set(long[kind])|set(short[kind])
        row[kind]={k:{m:(long[kind].get(k,{}).get(m,0)-short[kind].get(k,{}).get(m,0))/(hi-lo) for m in ['Ir','Bcm']} for k in keys}
    costs.append(row)
(directory/'attribution.json').write_text(json.dumps(dict(marginal=costs,reader_arenas=arena),indent=2)+'\n')
report['categories']=[{k:v for k,v in row.items() if k not in ['files','functions']} for row in costs]
report['hot_files']=[{k:v for k,v in row.items() if k not in ['files','functions','categories']}|dict(files=sorted(row['files'].items(),key=lambda kv:kv[1]['Ir'],reverse=True)[:10]) for row in costs]
report['hot_functions']=[{k:row[k] for k in ['transport','case','variant','peer']}|dict(functions=sorted(row['functions'].items(),key=lambda kv:kv[1]['Ir'],reverse=True)[:10]) for row in costs]
(directory/'summary.json').write_text(json.dumps(report,indent=2)+'\n')
print('ATTRIBUTION_SAVED', directory/'attribution.json', flush=True)
assert args.saved_root or run['conclusion']=='success', 'measurement or cleanup failed; retained evidence is diagnostic only'

# Full allocation stacks support attribution beyond one preselected arena.
# Normalize instruction addresses and checkout prefixes before subtracting runs.
def allocation_key(frames):
    return tuple(normalize(re.sub(r'^0x[0-9A-Fa-f]+: ', '', frame)) for frame in frames)
def allocation_counts(row):
    result = collections.defaultdict(collections.Counter)
    data=json.loads(row['source'].read_text())
    for site in data['pps']:
        frames=[data['ftbl'][i] for i in site['fs']]
        result[allocation_key(frames)].update(blocks=site['tbk'],bytes=site['tb'])
    return result
allocation_index={(r['transport'],r['case'],r['variant'],r['iterations'],r['peer']):r for r in allocation_sites}
allocation_output=(directory/'allocation-sites.json').open('w')
allocation_output.write('[\n')
first_allocation=True
for key,long in allocation_index.items():
    lo,hi=long['points']
    if key[3]!=hi: continue
    short=allocation_index.get((*key[:3],lo,key[4]))
    if short is None: continue
    first,last=allocation_counts(short),allocation_counts(long)
    sites=[dict(frames=frames, **{m:(last[frames][m]-first[frames][m])/(hi-lo) for m in ['blocks','bytes']}) for frames in first.keys()|last.keys()]
    if not first_allocation: allocation_output.write(',\n')
    json.dump({k:long[k] for k in ['transport','case','variant','peer']} | dict(sites=sorted(sites,key=lambda s:s['blocks'],reverse=True)),allocation_output,indent=2)
    first_allocation=False
allocation_output.write('\n]\n')
allocation_output.close()
