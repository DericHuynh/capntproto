"""Reconcile readiness/scheduler self work and compare retained disassembly."""
import json
from pathlib import Path
import re
import statistics
import sys

root = Path(sys.argv[1])
comparison = json.loads((root / 'comparison.json').read_text())
functions = json.loads((root / 'functions-selected.json').read_text())
scope = json.loads((root / 'scope.json').read_text())
assert len(scope['trials']) == 240 and not scope['failures']
lookup = {r['case']: r for r in comparison['comparisons'] if r['mode'] == 'callgrind'}


def is_readiness(name):
    return 'capnp_rpc::system::registry' in name


work = []
for row in functions:
    total = lookup[row['case']]['metrics'][row['metric']]['paired_delta']['mean']
    assert abs(row['attributed_delta'] - total) < 1e-6
    for group in ['registry', 'registry_and_scheduler']:
        paths = [p for p in row['selected_paths'] if is_readiness(p['function'])
                 or (group == 'registry_and_scheduler' and any(
                     x in p['function'] for x in ['task_set::', 'capntproto_rpc_engine::scheduler']))]
        a = [sum(p['baseline']['samples'][i] for p in paths) for i in range(5)]
        b = [sum(p['candidate']['samples'][i] for p in paths) for i in range(5)]
        delta = [y - x for x, y in zip(a, b)]
        work.append(dict(case=row['case'], metric=row['metric'], group=group,
                         baseline=a, candidate=b, paired_delta=delta,
                         mean_delta=statistics.mean(delta), sd_delta=statistics.stdev(delta)))


def assembly(path):
    result = {}
    name = None
    for line in path.read_text().splitlines():
        header = re.match(r'^([0-9a-f]+) <(.*)>:$', line)
        if header:
            address, name = header.groups()
            result[name] = dict(address=int(address, 16), instructions=[], indirect_calls=[])
        elif name:
            match = re.match(r'^\s*([0-9a-f]+):\s+(.*)$', line)
            if match:
                address, instruction = match.groups()
                address = int(address, 16)
                instruction = instruction.split('#', 1)[0].strip()
                instruction = re.sub(r'-?0x[0-9a-f]+\(%rip\)', '<RIP-relative>', instruction)
                instruction = re.sub(r'\b[0-9a-f]+ (<.*>)', r'\1', instruction)
                result[name]['instructions'].append([address - result[name]['address'], instruction])
                if re.match(r'call\s+\*', instruction):
                    result[name]['indirect_calls'].append(dict(address=address, predictor_index=address & 511))
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
result = dict(self_work=work,
              assembly_method='Instruction offsets/forms with linked addresses and RIP displacements normalized. Not byte equivalence.',
              assembly=layout, added_symbols=sorted(after.keys() - before.keys()),
              removed_symbols=sorted(before.keys() - after.keys()))
(root / 'analysis.json').write_text(json.dumps(result, indent=2) + '\n')


def symbols(label):
    rows = []
    for line in (root / (label + '.symbols')).read_text().splitlines():
        match = re.fullmatch(r'([0-9a-f]+) (?:([0-9a-f]+) )?([A-Za-z]) (.*)', line)
        if match:
            rows.append((line, match.groups()))
    return rows


symbol_rows = {label: symbols(label) for label in ['factored', 'candidate']}
text = {label: [line for line, (_, _, kind, _) in rows if kind in ['t', 'T']]
        for label, rows in symbol_rows.items()}
examples = ['capnp::private::layout::NULL_ARENA',
            'capntproto_rpc_wire::rpc_capnp::cap_descriptor::_private::ENCODED_NODE']
symbol_analysis = dict(text_symbol_rows=len(text['factored']),
                       text_symbol_rows_identical=text['factored'] == text['candidate'],
                       read_only_examples={label: {name: address for _, (address, _, _, name) in rows
                                                  if name in examples}
                                           for label, rows in symbol_rows.items()},
                       scope='nm -nSC text-symbol names, addresses and sizes; not instruction-byte equivalence.')
(root / 'symbol-analysis.json').write_text(json.dumps(symbol_analysis, indent=2) + '\n')
for row in work:
    if row['metric'] in ['Ir', 'Bi']:
        print(row['case'], row['group'], row['metric'], row['mean_delta'], 'SD', row['sd_delta'])
