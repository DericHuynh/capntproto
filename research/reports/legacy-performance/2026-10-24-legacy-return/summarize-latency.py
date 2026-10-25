"""Retain completed Return-ownership latency, including regressions and spread."""
import argparse
import json
import math
from pathlib import Path
import zipfile

parser=argparse.ArgumentParser(description=__doc__)
parser.add_argument('artifact',type=Path)
parser.add_argument('output',type=Path)
args=parser.parse_args()
args.output.mkdir(parents=True,exist_ok=True)
with zipfile.ZipFile(args.artifact) as archive:
    data=json.loads(archive.read('native-summary.json'))
    (args.output/'latency-summary.json').write_bytes(archive.read('native-summary.json'))
    (args.output/'latency-provenance.json').write_bytes(archive.read('provenance.json'))
assert data['measurement_outcome']=='success' and data['native_trials']==1080
assert len(data['latency'])==48
rows=[]
for row in data['latency']:
    variants=row['variants']
    for value in variants.values():
        assert len(value['repetition_mean_ns'])==5 and value['samples']==5000
    before,after=variants['legacy-baseline'],variants['legacy']
    rows.append(dict(transport=row['transport'],case=row['case']['name'],
                     legacy_change_percent=100*(after['mean_ns']/before['mean_ns']-1),
                     portable_control_change_percent=100*(variants['portable']['mean_ns']/variants['portable-baseline']['mean_ns']-1),
                     variants=variants))
aggregate={}
for transport in ['tls','quic']:
    selected=[r for r in rows if r['transport']==transport]
    aggregate[transport]=dict(cases=len(selected),lower_means=sum(r['legacy_change_percent']<0 for r in selected),
                              geometric_mean_change_percent=100*(math.exp(sum(math.log1p(r['legacy_change_percent']/100) for r in selected)/len(selected))-1))
result=dict(run=40140249948,candidate='6904a4ce5dda49470303b586f4d2b87a7a25dc47',
            baseline='a5677a63bb13273dff614205367f944a00145acf',
            scope='Dedicated droplets; five fresh processes and 1000 retained samples per repetition. '
                  'Mean +/- sample SD of repetition means; not a confidence interval. All cases retained.',
            aggregate=aggregate,host_counters=data['host_counters'],rows=rows)
(args.output/'latency-comparison.json').write_text(json.dumps(result,indent=2)+'\n')
lines=['# Dedicated latency after Return-ownership extraction','',
       result['scope']+' Values below are microseconds. The controls still build in separate source paths in this reusable latency workflow. '
       'This run does not contain the later idle-task scheduler candidate.','',
       '| Transport | Case | Frozen legacy | Factored legacy | Change | Portable | C++ |',
       '| --- | --- | ---: | ---: | ---: | ---: | ---: |']
def cell(value):
    return f"{value['mean_ns']/1000:.2f} ± {value['repetition_mean_stddev_ns']/1000:.2f}"
for row in rows:
    v=row['variants']
    lines.append(f"| {row['transport']} | {row['case']} | {cell(v['legacy-baseline'])} | {cell(v['legacy'])} | "
                 f"{row['legacy_change_percent']:+.2f}% | {cell(v['portable'])} | {cell(v['cpp']) if 'cpp' in v else '—'} |")
(args.output/'latency.md').write_text('\n'.join(lines)+'\n')
print(json.dumps(aggregate,indent=2))
