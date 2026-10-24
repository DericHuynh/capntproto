"""Summarize self-instruction differences from paired Callgrind captures."""
import argparse
from collections import defaultdict
import json
from pathlib import Path
import re
import zipfile

parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('artifact',type=Path)
parser.add_argument('output',type=Path)
args=parser.parse_args()
args.output.mkdir(parents=True,exist_ok=True)

def parse(text):
    names={}
    current=None
    skip=False
    counts=defaultdict(int)
    events=positions=summary=totals=None
    for line in text.splitlines():
        if line.startswith('events:'): events=line.split()[1:]
        elif line.startswith('positions:'): positions=len(line.split())-1
        elif line.startswith('summary:'): summary=list(map(int,line.split()[1:]))
        elif line.startswith('totals:'): totals=list(map(int,line.split()[1:]))
        elif line.startswith(('fn=','cfn=')):
            match=re.match(r'(c?fn)=\((\d+)\)(?: (.*))?$',line)
            assert match,line
            kind,index,name=match.groups()
            if name is not None: names[index]=name
            if kind=='fn': current=index
        elif line.startswith('calls='): skip=True
        elif line and line[0] in '0123456789+-*':
            if skip: skip=False;continue
            values=line.split()[positions:]
            assert current is not None
            counts[current]+=int(values[events.index('Ir')])
    assert sum(counts.values())==totals[events.index('Ir')],(sum(counts.values()),totals[events.index('Ir')])
    result=defaultdict(int)
    for index,count in counts.items():
        name=names[index]
        if re.fullmatch(r'0x[0-9a-fA-F]+',name): name='[unresolved instruction address]'
        result[name]+=count
    result['[summary minus attributed total]']+=summary[events.index('Ir')]-totals[events.index('Ir')]
    return result

with zipfile.ZipFile(args.artifact) as archive:
    scope=json.loads(archive.read('scope.json'))
    comparison=json.loads(archive.read('comparison.json'))
    measured={}
    for trial in scope['trials']:
        if trial['mode']!='callgrind': continue
        counts=defaultdict(int)
        for peer in ['client',*[f"service-{s['hop']}" for s in trial['services']]]:
            values=parse(archive.read(f"{trial['path']}/{peer}.callgrind").decode())
            for name,count in values.items(): counts[name]+=count
        measured[trial['case'],trial['implementation'],trial['iterations']]=counts
    slopes={}
    for key,large in measured.items():
        if key[-1]!=50:continue
        small=measured[(*key[:-1],10)]
        result={name:(large.get(name,0)-small.get(name,0))/40 for name in large.keys()|small.keys()}
        row=next(r for r in comparison['rows'] if r['mode']=='callgrind' and (r['case'],r['implementation'])==key[:-1])
        assert abs(sum(result.values())-row['counts']['Ir'])<1e-4
        slopes[key[:-1]]=result
    rows=[]
    for key,after in slopes.items():
        if key[-1]!='legacy': continue
        before=slopes[key[0],'legacy-baseline']
        changes=[dict(function=n,candidate=after.get(n,0),baseline=before.get(n,0),delta=after.get(n,0)-before.get(n,0)) for n in after.keys()|before.keys() if abs(after.get(n,0)-before.get(n,0))>1e-8]
        changes.sort(key=lambda r:(-abs(r['delta']),r['function']))
        selected=[dict(function=n,candidate=after.get(n,0),baseline=before.get(n,0),delta=after.get(n,0)-before.get(n,0)) for n in sorted(after.keys()|before.keys()) if any(fragment in n for fragment in ['ReturnGuard', 'ReturnOwnership', 'answer_has_sent_return', 'AnswerTaskDriver', 'ResultsInner'])]
        rows.append(dict(case=key[0],total_delta=sum(r['delta'] for r in changes),selected_return_paths=selected,changes=changes))
    data={'method':'All-peer paired (50-10)/40 self-instruction slopes. Each raw file self-cost sum is checked against its totals line, and any summary-minus-total residual is retained in an explicit bucket; slopes are checked against comparison.json. Symbol names retained, unresolved instruction addresses grouped. Codegen/inlining can move self-cost between functions; a symbol delta alone is not a causal attribution.','rows':rows}
    (args.output/'instruction-functions.json').write_text(json.dumps(data,indent=2)+'\n')
    for row in rows: print(row['case'],round(row['total_delta'],3),len(row['changes']),'changed function names')
