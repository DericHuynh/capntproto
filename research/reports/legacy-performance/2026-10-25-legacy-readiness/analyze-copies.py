from collections import Counter,defaultdict
import json,re,sys
from pathlib import Path
root=Path(sys.argv[1]); output=Path(sys.argv[2])
def parse(path):
 names={};current=target=None;call=None;position=0;lines=defaultdict(Counter);calls=Counter();events=[]
 for line in path.read_text().splitlines():
  if line.startswith('events:'):events=line.split()[1:]
  elif line.startswith(('fn=','cfn=')):
   m=re.fullmatch(r'(c?fn)=\((\d+)\)(?: (.*))?',line);kind,index,name=m.groups()
   if name is not None:names[index]=name
   if kind=='fn':current=index;position=0
   else:target=index
  elif line.startswith('calls='):
   count=int(line.split()[0][6:]);calls[current,target]+=count;call=True
  elif line and line[0] in '0123456789+-*':
   tokens=line.split();p=tokens[0]
   if p!='*':position=position+int(p) if p[0] in '+-' else int(p)
   if call:call=None;continue
   values=list(map(int,tokens[1:]));values += [0]*(len(events)-len(values))
   lines[current,position].update(dict(zip(events,values)))
 wanted='__memcpy_avx_unaligned_erms'
 # Multiple contexts of one caller may share its name.
 selected=Counter()
 for (a,b),n in calls.items():
  if names[b]==wanted:selected[names[a]]+=n
 selected_lines={p:dict(v) for (f,p),v in lines.items() if names[f]==wanted}
 return dict(calls=dict(selected),lines=selected_lines)
rows=[];captured={}
for variant in ['factored','candidate']:
 for iterations in [10,50]:
  for peer in ['client','service-0']:
   captured[variant,iterations,peer]=parse(root/f'callgrind-tls-deep-32-concurrent-32-{variant}-0-{iterations}'/(peer+'.callgrind'))
for peer in ['client','service-0']:
 slopes={}
 for variant in ['factored','candidate']:
  low,high=[captured[variant,n,peer] for n in [10,50]]
  slopes[variant]=dict(calls={k:(high['calls'].get(k,0)-low['calls'].get(k,0))/40 for k in high['calls'].keys()|low['calls'].keys()},
   lines={p:{k:(high['lines'].get(p,{}).get(k,0)-low['lines'].get(p,{}).get(k,0))/40 for k in ['Ir','Bc','Bcm','Bi','Bim']} for p in high['lines'].keys()|low['lines'].keys()})
 a,b=slopes['factored'],slopes['candidate']
 changes={k:b['calls'].get(k,0)-a['calls'].get(k,0) for k in a['calls'].keys()|b['calls'].keys()}
 changed_lines=[dict(line=p,baseline=a['lines'].get(p,{}),candidate=b['lines'].get(p,{}),delta={k:b['lines'].get(p,{}).get(k,0)-a['lines'].get(p,{}).get(k,0) for k in ['Ir','Bc','Bcm','Bi','Bim']}) for p in sorted(a['lines'].keys()|b['lines'].keys())]
 changed_lines=[x for x in changed_lines if any(x['delta'].values())]
 rows.append(dict(peer=peer,baseline_call_slope=sum(a['calls'].values()),candidate_call_slope=sum(b['calls'].values()),caller_call_deltas={k:v for k,v in changes.items() if v},line_changes=changed_lines))
functions=json.loads((output.parent/'functions-selected.json').read_text())
row=next(r for r in functions if r['case']=='deep-32-concurrent-32' and r['metric']=='Ir')
expected=next(f for f in row['top_changes'] if f['function']=='__memcpy_avx_unaligned_erms')['paired_delta']['samples'][0]
assert abs(sum(sum(x['delta']['Ir'] for x in p['line_changes']) for p in rows)-expected)<1e-6
output.write_text(json.dumps(dict(scope='First repetition only; libc memcpy call arcs and assembly-source self costs, (50-10)/40.',source='glibc memmove-vec-unaligned-erms.S',rows=rows),indent=2)+'\n')
for r in rows:
 print(r['peer'],'calls',r['baseline_call_slope'],r['candidate_call_slope'],'call deltas',r['caller_call_deltas'])
 print('Ir/Bc line deltas',[(x['line'],x['delta']['Ir'],x['delta']['Bc']) for x in r['line_changes'] if x['delta']['Ir'] or x['delta']['Bc']])
