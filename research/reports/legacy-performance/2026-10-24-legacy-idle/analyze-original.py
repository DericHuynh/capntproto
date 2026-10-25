"""Attribute the original single-pair table; preserve every counter and residual."""
import argparse
from collections import Counter, defaultdict
import json
from pathlib import Path
import sys
import zipfile

sys.path.insert(0, str(Path(__file__).resolve().parents[4] / 'benchmarks/rpc'))
from work_profile import callgrind, slope

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('artifact', type=Path)
parser.add_argument('output', type=Path)
args = parser.parse_args()
with zipfile.ZipFile(args.artifact) as archive:
    scope = json.loads(archive.read('scope.json'))
    comparison = json.loads(archive.read('comparison.json'))
    totals, functions = {}, {}
    for trial in scope['trials']:
        if trial['mode'] != 'callgrind':
            continue
        key = trial['case'], trial['implementation'], trial['iterations']
        total, names = Counter(), defaultdict(Counter)
        for peer in ['client', *[f"service-{s['hop']}" for s in trial['services']]]:
            counts, costs = callgrind(archive.read(f"{trial['path']}/{peer}.callgrind").decode())
            total.update(counts)
            for name, values in costs.items():
                names[name].update(values)
        totals[key], functions[key] = total, names
    slopes, costs = {}, {}
    for key, large in totals.items():
        if key[-1] != 50:
            continue
        small_key = (*key[:-1], 10)
        slopes[key[:-1]] = slope(totals[small_key], large, [10, 50])
        row = next(r for r in comparison['rows'] if r['mode'] == 'callgrind'
                   and (r['case'], r['implementation']) == key[:-1])
        for event, count in slopes[key[:-1]].items():
            assert abs(row['counts'][event] - count) < 1e-5
        before, after = functions[small_key], functions[key]
        costs[key[:-1]] = {name: slope(before.get(name, {}), after.get(name, {}), [10, 50])
                          for name in before.keys() | after.keys()}
    rows = []
    for case, label in slopes:
        if label != 'legacy':
            continue
        before, after = costs[case, 'legacy-baseline'], costs[case, 'legacy']
        for event in ['Ir', 'Bc', 'Bcm', 'Bi', 'Bim']:
            changes = [dict(function=name, baseline=before.get(name, {}).get(event, 0),
                            candidate=after.get(name, {}).get(event, 0),
                            delta=after.get(name, {}).get(event, 0) - before.get(name, {}).get(event, 0))
                       for name in before.keys() | after.keys()]
            changes = [r for r in changes if r['delta']]
            changes.sort(key=lambda r: (-abs(r['delta']), r['function']))
            delta = slopes[case, 'legacy'][event] - slopes[case, 'legacy-baseline'][event]
            assert abs(sum(r['delta'] for r in changes) - delta) < 1e-5
            rows.append(dict(case=case, event=event, baseline=slopes[case, 'legacy-baseline'][event],
                             candidate=slopes[case, 'legacy'][event], delta=delta, changes=changes))
    args.output.write_text(json.dumps(dict(source_run=40140249948, source_artifact=12579116027,
                                         method='Paired all-peer self costs; all event totals reconciled, call arc costs excluded, '
                                                'trailing zero counters restored, residual startup costs retained.',
                                         rows=rows), indent=2) + '\n')
