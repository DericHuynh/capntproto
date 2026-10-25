#!/usr/bin/env python3
"""Repeat the legacy work comparison, optionally with frozen and identical controls."""
import argparse
from collections import Counter, defaultdict
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import re
import subprocess
import sys

from work_profile import callgrind, comparisons, slope, spread

HERE = Path(__file__).resolve().parent
PAIRS = [('frozen', 'factored'), ('factored', 'repeat'), ('factored', 'candidate')]
CASES = ['unary-small', 'dependent-read', 'pipelined-read', 'concurrent-32', 'callback', 'deep-32-concurrent-32']


def save(path, value):
    path.write_text(json.dumps(value, indent=2) + '\n')


def assembly(path, output):
    # Keep addresses and instructions for the changed scheduler and unchanged
    # guard/TLS hot paths; this diagnoses layout changes without huge binaries.
    selected = ('task_set::TaskSet', 'task_set::ready_set::Ready',
                'task_set::ready_set::Notify', 'capntproto_rpc_engine::readiness',
                'ReturnGuard', 'answer_has_sent_return',
                'aes_gcm::aeshwclmulmovbe::seal', '::poll_prepare_frame')
    with output.open('w') as target:
        process = subprocess.Popen(['objdump', '-Cd', '--no-show-raw-insn', str(path)],
                                   stdout=subprocess.PIPE, text=True)
        keep = False
        for line in process.stdout:
            if re.match(r'^[0-9a-f]+ <', line):
                keep = any(name in line for name in selected)
            if keep:
                target.write(line)
        assert process.wait() == 0


def summarize(output):
    scope = json.loads((output / 'scope.json').read_text())
    pairs = scope.get('pairs', PAIRS)
    assert not scope['failures'], scope['failures']
    expected = scope['repetitions'] * len(scope['binaries']) * 24
    assert len(scope['trials']) == expected, (len(scope['trials']), expected)
    values, functions = {}, {}
    for trial in scope['trials']:
        key = trial['mode'], trial['transport'], trial['case'], trial['variant'], trial['repetition'], trial['iterations']
        assert key not in values
        counts, names = Counter(), defaultdict(Counter)
        for peer in ['client', *[f"service-{s['hop']}" for s in trial['services']]]:
            base = output / trial['path'] / peer
            if trial['mode'] == 'callgrind':
                totals, costs = callgrind(base.with_suffix('.callgrind').read_text())
                counts.update(totals)
                for name, events in costs.items():
                    names[name].update(events)
            else:
                data = json.loads(base.with_suffix('.dhat.json').read_text())
                counts.update(blocks=sum(p['tbk'] for p in data['pps']),
                              bytes=sum(p['tb'] for p in data['pps']))
        values[key], functions[key] = counts, names
    rows, per_function = [], {}
    low, high = scope['points']
    for key, large in values.items():
        if key[-1] != high:
            continue
        small = values[(*key[:-1], low)]
        counts = slope(small, large, [low, high])
        rows.append(dict(mode=key[0], transport=key[1], case=key[2], variant=key[3],
                         repetition=key[4], counts=counts))
        if key[0] == 'callgrind':
            before, after = functions[(*key[:-1], low)], functions[key]
            delta = {name: slope(before.get(name, {}), after.get(name, {}), [low, high])
                     for name in before.keys() | after.keys()}
            for metric, count in counts.items():
                assert abs(sum(d.get(metric, 0) for d in delta.values()) - count) < 1e-5
            per_function[key[2], key[3], key[4]] = delta
    result = dict(scope='All peers; five fresh-process pairs per variant; (50-10)/40 with two warmups. '
                        'Sample SD describes paired slopes, including startup variation. Simulated branches; no latency claim.',
                  rows=rows, comparisons=comparisons(rows, pairs))
    save(output / 'comparison.json', result)
    attribution = []
    for case in CASES:
        for left, right in pairs:
            before = [per_function[case, left, r] for r in range(scope['repetitions'])]
            after = [per_function[case, right, r] for r in range(scope['repetitions'])]
            names = set().union(*(row.keys() for row in [*before, *after]))
            for metric in ['Ir', 'Bc', 'Bcm', 'Bi', 'Bim']:
                changes = []
                for name in sorted(names):
                    a = [row.get(name, {}).get(metric, 0) for row in before]
                    b = [row.get(name, {}).get(metric, 0) for row in after]
                    # Include zero-delta hot functions as well as changes.
                    if any(a) or any(b):
                        changes.append(dict(function=name, baseline=spread(a), candidate=spread(b),
                                            paired_delta=spread([y-x for x,y in zip(a,b)])))
                changes.sort(key=lambda r: (-abs(r['paired_delta']['mean']), r['function']))
                attribution.append(dict(case=case, baseline=left, candidate=right, metric=metric,
                                        functions=changes))
    save(output / 'functions.json', attribution)
    lines = ['# Repeated legacy work comparison', '',
             'Five repetitions, mean ± sample SD. Paired 50−10 slopes retain startup noise; branch events are simulated. '
             'No latency claim.', '']
    if 'repeat' in scope['binaries']:
        lines += ['The repeat control uses the exact factored executable and path.', '']
    for left, right in pairs:
        lines += [f'## {right} versus {left}', '',
                  '| Transport | Case | Metric | Before | After | Change | Paired delta SD |',
                  '| --- | --- | --- | ---: | ---: | ---: | ---: |']
        for row in result['comparisons']:
            if (row['baseline'], row['candidate']) != (left, right):
                continue
            for metric in (['Ir', 'Bc', 'Bcm', 'Bi', 'Bim'] if row['mode'] == 'callgrind' else ['blocks', 'bytes']):
                values = row['metrics'][metric]
                a, b = values['baseline'], values['candidate']
                pct = values['change_percent']
                change = f'{pct:+.3f}%' if pct is not None else 'n/a'
                lines.append(f"| {row['transport']} | {row['case']} | {metric} | "
                             f"{a['mean']:,.3f} ± {a['sd']:,.3f} | {b['mean']:,.3f} ± {b['sd']:,.3f} | "
                             f"{change} | {values['paired_delta']['sd']:,.3f} |")
        lines.append('')
    (output / 'comparison.md').write_text('\n'.join(lines))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ['frozen', 'factored', 'candidate']:
        parser.add_argument('--' + name, type=Path)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--summarize-only', action='store_true')
    parser.add_argument('--pair-only', action='store_true',
                        help='Compare factored and candidate only, after separate control calibration.')
    args = parser.parse_args()
    output = args.output.resolve()
    if args.summarize_only:
        summarize(output)
        return
    labels = ['factored', 'candidate'] if args.pair_only else ['frozen', 'factored', 'repeat', 'candidate']
    assert all(getattr(args, label) for label in labels if label != 'repeat')
    spec = importlib.util.spec_from_file_location('secure_profile', HERE / 'secure-profile.py')
    profile = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(profile)
    profile.r.idle_host()
    output.mkdir(parents=True, exist_ok=False)
    paths = {label: getattr(args, label).resolve(strict=True) for label in labels if label != 'repeat'}
    if not args.pair_only:
        paths['repeat'] = paths['factored']
    certs = profile.r.certificates.generate(output / 'certificates')
    cpus, topology = profile.r.physical_cpus()
    diagnostic = profile.Diagnostic({'legacy': paths['factored']}, output, certs, cpus[:4])
    scope = dict(repetitions=5, points=[10, 50], warmup=2, trials=[], failures=[],
                 pairs=[('factored', 'candidate')] if args.pair_only else PAIRS,
                 cpus=cpus[:4], cpu_topology=topology, binaries={},
                 workflow_sha=os.environ.get('GITHUB_SHA'),
                 method='Rotate variant order each repetition and reverse short/long order on alternate repetitions. '
                        'All peers use legacy; when present, repeat and factored use an identical executable path. '
                        'Cryptographic randomness is not replaced or seeded.')
    for label, path in paths.items():
        scope['binaries'][label] = dict(path=str(path), sha256=hashlib.file_digest(path.open('rb'), 'sha256').hexdigest(),
                                        sections=subprocess.check_output(['size', '-A', str(path)], text=True))
        if label != 'repeat':
            (output / (label + '.symbols')).write_text(subprocess.check_output(['nm', '-nSC', str(path)], text=True))
            assembly(path, output / (label + '.assembly'))
    save(output / 'scope.json', scope)
    for mode in ['callgrind', 'dhat']:
        diagnostic.mode = mode
        for transport in (['tls'] if mode == 'callgrind' else ['tls', 'quic']):
            for name in (CASES if mode == 'callgrind' else ['unary-small', 'pipelined-read', 'concurrent-32']):
                case = next(c for c in profile.r.catalog() if c['name'] == name)
                for repetition in range(5):
                    order = labels[repetition % len(labels):] + labels[:repetition % len(labels)]
                    for label in order:
                        diagnostic.binaries['legacy'] = paths[label]
                        for iterations in ([10, 50] if repetition % 2 == 0 else [50, 10]):
                            prefix = f'{mode}-{transport}-{name}-{label}-{repetition}-{iterations}'
                            try:
                                trial = diagnostic.trial(case, 'legacy', transport, prefix, 2, iterations)
                                scope['trials'].append(dict(path=prefix, mode=mode, transport=transport,
                                                           case=name, variant=label, repetition=repetition,
                                                           iterations=iterations, services=trial['services']))
                                print(prefix, flush=True)
                            except Exception as error:
                                scope['failures'].append(dict(path=prefix, error=f'{type(error).__name__}: {error}'))
                                print(f'FAILED {prefix}: {error}', flush=True)
                            save(output / 'scope.json', scope)
    summarize(output)


if __name__ == '__main__':
    main()
