"""Reconcile selected scheduler work and retain a separately measured frozen reference."""
import json
from pathlib import Path
import re
import statistics
import sys

root, calibration = map(Path, sys.argv[1:])
comparison = json.loads((root / 'comparison.json').read_text())
functions = json.loads((root / 'functions-selected.json').read_text())
previous = json.loads((calibration / 'comparison.json').read_text())
scope = json.loads((root / 'scope.json').read_text())
control = json.loads((calibration / 'scope.json').read_text())
assert scope['binaries']['factored']['sha256'] == control['binaries']['factored']['sha256']
assert len(scope['trials']) == 240 and not scope['failures']
lookup = {r['case']: r for r in comparison['comparisons'] if r['mode'] == 'callgrind'}
work = []
for row in functions:
    total = lookup[row['case']]['metrics'][row['metric']]['paired_delta']['mean']
    assert abs(row['attributed_delta'] - total) < 1e-6
    if row['metric'] not in ['Ir', 'Bi']:
        continue
    paths = [p for p in row['selected_paths']
             if 'task_set::TaskSet' in p['function'] or 'task_set::Activity' in p['function']]
    samples = [sum(p['paired_delta']['samples'][i] for p in paths) for i in range(5)]
    work.append(dict(case=row['case'], metric=row['metric'], samples=samples,
                     mean=statistics.mean(samples), sd=statistics.stdev(samples)))

reference = []
for case, row in lookup.items():
    frozen = next(r for r in previous['comparisons']
                  if r['baseline'] == 'frozen' and r['mode'] == 'callgrind' and r['case'] == case)
    metrics = {}
    for metric in ['Ir', 'Bi', 'Bcm', 'Bim']:
        before = frozen['metrics'][metric]['baseline']
        after = row['metrics'][metric]['candidate']
        metrics[metric] = dict(frozen=before, candidate=after,
                               change_percent=100 * (after['mean'] / before['mean'] - 1))
    reference.append(dict(case=case, metrics=metrics))


def assembly(path):
    result = {}
    name = None
    for line in path.read_text().splitlines():
        header = re.match(r'^([0-9a-f]+) <(.*)>:$', line)
        if header:
            address, name = header.groups()
            result[name] = dict(address=int(address, 16), instructions=[], indirect_calls=[], indirect_jumps=[])
        elif name:
            match = re.match(r'^\s*([0-9a-f]+):\s+(.*)$', line)
            if match:
                address, instruction = match.groups()
                address = int(address, 16)
                instruction = instruction.split('#', 1)[0].strip()
                instruction = re.sub(r'-?0x[0-9a-f]+\(%rip\)', '<RIP-relative>', instruction)
                instruction = re.sub(r'\b[0-9a-f]+ (<.*>)', r'\1', instruction)
                result[name]['instructions'].append([address - result[name]['address'], instruction])
                for op, key in [('call', 'indirect_calls'), ('jmp', 'indirect_jumps')]:
                    if re.match(op + r'\s+\*', instruction):
                        result[name][key].append(dict(address=address, predictor_index=address & 511))
    return result


before, after = [assembly(root / (label + '.assembly')) for label in ['factored', 'candidate']]
layout = []
for name in sorted(before.keys() & after.keys()):
    a, b = before[name], after[name]
    layout.append(dict(function=name, factored_address=a['address'], candidate_address=b['address'],
                       normalized_structure_equal=a['instructions'] == b['instructions'],
                       instruction_count=[len(a['instructions']), len(b['instructions'])],
                       indirect_call_count=[len(a['indirect_calls']), len(b['indirect_calls'])],
                       factored_indirect_calls=a['indirect_calls'], candidate_indirect_calls=b['indirect_calls']))

result = dict(scheduler_and_waker_self_work=work,
              frozen_reference_method='Separately measured five-repeat frozen control, not a paired candidate/frozen trial. '
                                      'The factored control executable hash matches across runs. No cross-run paired SD is fabricated.',
              frozen_reference=reference,
              assembly_method='Instruction offsets/forms with linked addresses and RIP displacements normalized. Not byte equivalence.',
              assembly=layout)
(root / 'analysis.json').write_text(json.dumps(result, indent=2) + '\n')
for row in work:
    print(row['case'], row['metric'], row['mean'], 'SD', row['sd'])
