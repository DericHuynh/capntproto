#!/usr/bin/env python3
"""Run Apalache symbolic checks with explicit trace bounds and durable reports.

Safety checking does not establish the temporal properties in TLC configurations.
The archived TLC runner remains available as check_tlc.py.
"""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import sys
import time
import uuid

from check_tlc import CASES, MODULES
ROOT = Path(__file__).resolve().parent
PIN = json.loads((ROOT / 'tools/apalache.json').read_text())

def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

def sections(source):
    result = {}
    key = None
    for line in source.splitlines():
        line = line.split('\\*', 1)[0].strip()
        if not line:
            continue
        match = re.match(r'^(CONSTANTS?|SPECIFICATION|INIT|NEXT|INVARIANTS?|PROPERT(?:Y|IES)|CHECK_DEADLOCK)\b(.*)', line)
        if match:
            key = {'CONSTANT':'CONSTANTS','INVARIANT':'INVARIANTS','PROPERTY':'PROPERTIES'}.get(match[1],match[1])
            result.setdefault(key, [])
            if match[2].strip():
                result[key].append(match[2].strip())
        elif key:
            result[key].append(line)
    return result

def classify(code, log, expected, traces, mode):
    if re.search(r'TIMEOUT|timed out|solver.*unknown', log, re.I):
        return 'solver_inconclusive'
    if mode == 'typecheck':
        return 'typechecked' if code == 0 and 'Type checker [OK]' in log else 'checker_error'
    if expected:
        # Exactly one invariant is selected for negative controls/witnesses.
        if code == 12 and 'state invariant 0 violated.' in log and traces:
            return 'expected_counterexample'
        if code == 0 and 'The outcome is: NoError' in log:
            return 'counterexample_not_found_within_bound'
        return 'checker_error'
    if code == 0 and 'The outcome is: NoError' in log:
        return 'bounded_pass'
    if code == 12 and ('invariant' in log or 'temporal' in log):
        return 'violation'
    return 'checker_error'

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('cases', nargs='*', help='Default: all configurations')
    parser.add_argument('--list', action='store_true')
    parser.add_argument('--java', default=os.environ.get('JAVA','java'))
    parser.add_argument('--jar', type=Path, default=ROOT/'tools'/PIN['jar'])
    parser.add_argument('--length', type=int, default=10, help='Maximum Next transitions, not a state count')
    parser.add_argument('--timeout', type=int, default=600)
    parser.add_argument('--timeout-smt', type=int, default=60)
    parser.add_argument('--heap', default='4g')
    parser.add_argument('--mode', choices=('safety','typecheck','temporal'), default='safety')
    parser.add_argument('--reports-dir', type=Path, default=ROOT/'reports/apalache')
    args = parser.parse_args()
    if args.list:
        for name in sorted(CASES):
            expected = f'; expects {CASES[name]}' if CASES[name] else ''
            print(f'{name}: {MODULES[name]}{expected}')
        return 0
    if args.length < 0 or args.timeout < 1 or args.timeout_smt < 1:
        parser.error('length must be nonnegative; timeouts must be positive')
    if not re.fullmatch(r'[1-9][0-9]*[mMgG]',args.heap):
        parser.error('heap must be a positive integer followed by m or g')
    cases = args.cases or sorted(CASES)
    if set(cases)-CASES.keys():
        parser.error('Unknown cases: '+', '.join(sorted(set(cases)-CASES.keys())))
    jar = args.jar.expanduser().resolve()
    if not jar.is_file():
        parser.error('Apalache missing; run python3 tools/install_apalache.py')
    if digest(jar) != PIN['jar_sha256']:
        parser.error('Apalache jar differs from tools/apalache.json; update the pin explicitly')
    java_version = subprocess.check_output([args.java,'-version'],stderr=subprocess.STDOUT,text=True).strip()
    version = re.search(r'version "(\d+)',java_version)
    if not version or int(version[1]) < PIN['minimum_java_version']:
        parser.error('The pinned Apalache requires Java 21 or newer')
    reports = args.reports_dir.resolve()
    reports.mkdir(parents=True,exist_ok=True)
    inputs = list(ROOT.glob('*.tla'))+[Path(__file__),ROOT/'check_tlc.py',ROOT/'tools/apalache.json']
    inputs += [ROOT/'configs'/(name+'.cfg') for name in cases]
    hashes = {str(p.relative_to(ROOT)):digest(p) for p in inputs}
    report = {'backend':'Apalache','version':PIN['version'],'jar_sha256':digest(jar),
        'java_version':java_version,'mode':args.mode,'maximum_transitions':args.length,
        'timeout_seconds':args.timeout,'timeout_smt_seconds':args.timeout_smt,'heap':args.heap,
        'started_utc':datetime.now(timezone.utc).isoformat(),'complete':False,
        'input_sha256':hashes,'results':[],
        'semantics':'Bounded symbolic checking; no exhaustive distinct-state count or unbounded proof.'}
    def save():
        temp = reports/'checks.json.tmp'
        temp.write_text(json.dumps(report,indent=2)+'\n')
        temp.replace(reports/'checks.json')
    save()
    successful = {'bounded_pass','expected_counterexample','typechecked'}
    for index,name in enumerate(cases):
        cfg = sections((ROOT/'configs'/(name+'.cfg')).read_text())
        record = {'case':name,'module':MODULES[name],'status':'running',
                  'expected_violation':CASES[name], 'maximum_transitions':args.length,
                  'temporal_properties_declared':cfg.get('PROPERTIES',[]),
                  'temporal_properties_checked':False}
        report['results'].append(record)
        save()
        work = reports/f'{index:03d}-{name}'
        work.mkdir(exist_ok=True)
        inv = [CASES[name]] if CASES[name] else cfg.get('INVARIANTS',[])
        config = work/'model.cfg'
        wrapper = work/'ApalacheCase.tla'
        wrapper.write_text('---------------- MODULE ApalacheCase ----------------\nEXTENDS '+MODULES[name]+
                           '\nApaNext == '+cfg.get('NEXT',['Next'])[0]+' \\/ UNCHANGED vars\n'+
                           '=====================================================\n')
        record['wrapper_sha256'] = digest(wrapper)
        record['transition_relation'] = 'Next or stuttering, as in [][Next]_vars'
        config.write_text('INIT '+cfg.get('INIT',['Init'])[0]+'\nNEXT ApaNext\n'+
                          ('CONSTANTS\n'+'\n'.join(cfg['CONSTANTS'])+'\n' if cfg.get('CONSTANTS') else '')+
                          ('INVARIANTS\n'+'\n'.join(inv)+'\n' if inv else ''))
        record['generated_config_sha256'] = digest(config)
        if args.mode == 'temporal':
            # Never silently drop the fairness assumptions in LiveSpec.
            record['status'] = 'unsupported_fairness'
            record['detail'] = 'WF/ENABLED must be translated explicitly before temporal migration; use the preserved TLC workflow.'
            save()
            print(f'UNSUPPORTED {name}: {record["detail"]}',flush=True)
            continue
        run_dir = work/'artifacts'/uuid.uuid4().hex
        command = [args.java,'-Xmx'+args.heap,'-jar',str(jar),'--out-dir='+str(run_dir)]
        if args.mode == 'typecheck':
            command += ['typecheck',str(ROOT/(MODULES[name]+'.tla'))]
        else:
            command += ['check','--config='+str(config),'--init='+cfg.get('INIT',['Init'])[0],
                        '--next=ApaNext', '--length='+str(args.length),
                        '--no-deadlock','--discard-disabled=true',
                        '--timeout-smt='+str(args.timeout_smt),str(wrapper)]
        record['command'] = command
        record['log'] = str((work/'checker.log').relative_to(reports))
        save()
        started = time.monotonic()
        print(f'RUN {name}: {args.mode}, at most {args.length} transitions',flush=True)
        with (work/'checker.log').open('w') as output:
            process = subprocess.Popen(command,cwd=ROOT,stdout=output,stderr=subprocess.STDOUT,start_new_session=True)
            try:
                code = process.wait(timeout=args.timeout)
            except (subprocess.TimeoutExpired, KeyboardInterrupt) as exc:
                os.killpg(process.pid,signal.SIGTERM)
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    os.killpg(process.pid,signal.SIGKILL)
                    process.wait()
                record.update(status='timeout' if isinstance(exc,subprocess.TimeoutExpired) else 'interrupted',
                              elapsed_seconds=round(time.monotonic()-started,3))
                save()
                if isinstance(exc,KeyboardInterrupt):
                    return 130
                print(f'TIMEOUT {name}: incomplete',flush=True)
                continue
        log = (work/'checker.log').read_text()
        traces = sorted(str(p.relative_to(reports)) for p in run_dir.rglob('violation*.itf.json'))
        record.update(exit_code=code,status=classify(code,log,CASES[name],traces,args.mode),
                      counterexample_traces=traces,elapsed_seconds=round(time.monotonic()-started,3),
                      log_sha256=digest(work/'checker.log'))
        depths = re.findall(r'^State (\d+):',log,re.M)
        record['last_reported_state_index'] = max(map(int,depths)) if depths else None
        save()
        print(f'{record["status"].upper()} {name}',flush=True)
        if record['status'] not in successful:
            print(log[-2500:],file=sys.stderr)
    unchanged = hashes == {str(p.relative_to(ROOT)):digest(p) for p in inputs}
    report.update(complete=True,finished_utc=datetime.now(timezone.utc).isoformat(),sources_unchanged_during_run=unchanged)
    save()
    return 0 if unchanged and all(r['status'] in successful for r in report['results']) else 1

if __name__ == '__main__':
    raise SystemExit(main())
