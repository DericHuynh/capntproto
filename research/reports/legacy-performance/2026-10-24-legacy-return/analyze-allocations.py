"""Normalize paired DHAT stacks and verify their totals against the summary."""
import argparse
from collections import defaultdict
import json
from pathlib import Path
import re
import zipfile

parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('artifact',type=Path)
parser.add_argument('output',type=Path)
parser.add_argument('--normalize-rpc-impls',action='store_true',help='Group unstable compiler-assigned impl indices in capnp_rpc::rpc frames; retain the ungrouped report separately.')
args=parser.parse_args()
args.output.mkdir(parents=True,exist_ok=True)
with zipfile.ZipFile(args.artifact) as archive:
    scope=json.loads(archive.read('scope.json'))
    comparison=json.loads(archive.read('comparison.json'))
    stacks={}
    for trial in scope['trials']:
        if trial['mode']!='dhat': continue
        counts=defaultdict(lambda:[0,0])
        for peer in ['client',*[f"service-{s['hop']}" for s in trial['services']]]:
            data=json.loads(archive.read(f"{trial['path']}/{peer}.dhat.json"))
            def frame(index):
                label=re.sub(r'^0x[0-9A-Fa-f]+: ','',data['ftbl'][index])
                label=label.rsplit(' (',1)[0]
                if args.normalize_rpc_impls:
                    label=re.sub(r'capnp_rpc::rpc::\{impl#\d+\}', 'capnp_rpc::rpc::{impl#}', label)
                return label
            for point in data['pps']:
                stack=tuple(frame(index) for index in point['fs'])
                counts[stack][0]+=point['tbk']
                counts[stack][1]+=point['tb']
        stacks[trial['transport'],trial['case'],trial['implementation'],trial['iterations']]=counts
    slopes={}
    for key,large in stacks.items():
        if key[-1]!=50: continue
        small=stacks[(*key[:-1],10)]
        values={stack:[(large.get(stack,[0,0])[i]-small.get(stack,[0,0])[i])/40 for i in (0,1)] for stack in large.keys()|small.keys()}
        row=next(r for r in comparison['rows'] if r['mode']=='dhat' and (r['transport'],r['case'],r['implementation'])==key[:-1])
        for i,metric in enumerate(['blocks','bytes']):
            assert abs(sum(v[i] for v in values.values())-row['counts'][metric])<1e-5,(key,metric)
        slopes[key[:-1]]=values
    rows=[]
    for key,after in slopes.items():
        if key[-1]!='legacy': continue
        before=slopes[(*key[:-1],'legacy-baseline')]
        changes=[]
        for stack in after.keys()|before.keys():
            a,b=after.get(stack,[0,0]),before.get(stack,[0,0])
            delta=[a[i]-b[i] for i in (0,1)]
            if any(abs(x)>1e-8 for x in delta):
                changes.append(dict(blocks_delta=delta[0],bytes_delta=delta[1],candidate=dict(blocks=a[0],bytes=a[1]),baseline=dict(blocks=b[0],bytes=b[1]),stack=list(stack)))
        changes.sort(key=lambda x:(-abs(x['blocks_delta']),-abs(x['bytes_delta']),x['stack']))
        rows.append(dict(transport=key[0],case=key[1],blocks_delta=sum(c['blocks_delta'] for c in changes),bytes_delta=sum(c['bytes_delta'] for c in changes),changes=changes))
    result={'method':'Sum all peers, normalize instruction addresses and source locations in DHAT frames; preserve negative startup-subtracted slopes. Verify normalized stack sums against original allocation and byte totals. Single pairs are diagnostic, not repeated parity evidence.','rows':rows}
    result['compiler_rpc_impl_indices_normalized']=args.normalize_rpc_impls
    output_name='allocation-stacks-canonical.json' if args.normalize_rpc_impls else 'allocation-stacks.json'
    (args.output/output_name).write_text(json.dumps(result,indent=2)+'\n')
    for row in rows:
        print(row['transport'],row['case'],'blocks delta',round(row['blocks_delta'],4),'bytes delta',round(row['bytes_delta'],4),'changed stacks',len(row['changes']))
