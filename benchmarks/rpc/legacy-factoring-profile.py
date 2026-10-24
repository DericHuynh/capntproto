"""Compare legacy against a frozen legacy binary; diagnostics are not latency evidence."""
import argparse
from collections import Counter
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--rust', type=Path, required=True)
    parser.add_argument('--baseline', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    here = Path(__file__).resolve().parent
    sys.path.insert(0, str(here))
    spec = importlib.util.spec_from_file_location('secure_profile', here / 'secure-profile.py')
    profile = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(profile)
    profile.r.idle_host()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    bins = {'legacy': args.rust.resolve(), 'legacy-baseline': args.baseline.resolve()}
    certificates = profile.r.certificates.generate(output / 'certificates')
    diagnostic = profile.Diagnostic(bins, output, certificates, sorted(os.sched_getaffinity(0))[:4])
    scope = {'trials': [], 'failures': [], 'binaries': {}}
    for label, path in bins.items():
        scope['binaries'][label] = {'path': str(path), 'sha256': hashlib.file_digest(path.open('rb'), 'sha256').hexdigest()}
        scope['binaries'][label]['sections'] = subprocess.check_output(['size', '-A', str(path)], text=True)
    cases = ['unary-small', 'dependent-read', 'pipelined-read', 'concurrent-32', 'callback', 'deep-32-concurrent-32']
    for mode in ['callgrind', 'dhat']:
        diagnostic.mode = mode
        for transport in (['tls'] if mode == 'callgrind' else ['tls', 'quic']):
            for name in (cases if mode == 'callgrind' else ['unary-small', 'pipelined-read', 'concurrent-32']):
                case = next(item for item in profile.r.catalog() if item['name'] == name)
                for label in bins:
                    for iterations in [10, 50]:
                        prefix = f'{transport}-{name}-{mode}-{label}-{iterations}'
                        try:
                            trial = diagnostic.trial(case, label, transport, prefix, 2, iterations)
                            scope['trials'].append(dict(path=prefix, transport=transport, case=name, mode=mode, implementation=label, iterations=iterations, services=trial['services']))
                            print(prefix, flush=True)
                        except Exception as error:
                            scope['failures'].append(dict(path=prefix, error=str(error)))
                        (output / 'scope.json').write_text(json.dumps(scope, indent=2) + '\n')
    assert not scope['failures'], scope['failures']
    assert len(scope['trials']) == 48
    values = {}
    for trial in scope['trials']:
        counts = Counter()
        for peer in ['client', *[f"service-{s['hop']}" for s in trial['services']]]:
            if trial['mode'] == 'dhat':
                data = json.loads((output / trial['path'] / (peer + '.dhat.json')).read_text())
                counts.update(blocks=sum(p['tbk'] for p in data['pps']), bytes=sum(p['tb'] for p in data['pps']))
            else:
                names = events = None
                for line in (output / trial['path'] / (peer + '.callgrind')).read_text().splitlines():
                    if line.startswith('events:'):
                        names = line.split()[1:]
                    if line.startswith('summary:'):
                        events = list(map(int, line.split()[1:]))
                assert names is not None and events is not None and len(names) == len(events)
                counts.update(dict(zip(names, events)))
        values[trial['mode'], trial['transport'], trial['case'], trial['implementation'], trial['iterations']] = counts
    rows = []
    for key, after in values.items():
        if key[-1] != 50:
            continue
        before = values[(*key[:-1], 10)]
        rows.append(dict(mode=key[0], transport=key[1], case=key[2], implementation=key[3], counts={name: (value - before[name]) / 40 for name, value in after.items()}))
    (output / 'comparison.json').write_text(json.dumps({'scope': 'All peers; paired (50-10)/40 operations, two warmups. Single diagnostic pairs; no latency claim.', 'rows': rows}, indent=2) + '\n')
    lines = ['# Legacy factoring diagnostics', '', 'All peers; paired (50−10)/40 slopes, two warmups. Single diagnostic pairs, not latency evidence.', '', '| Transport | Case | Metric | Frozen legacy | Factored legacy | Change |', '| --- | --- | --- | ---: | ---: | ---: |']
    for row in rows:
        if row['implementation'] != 'legacy':
            continue
        control = next(r for r in rows if r['implementation'] == 'legacy-baseline' and all(r[k] == row[k] for k in ['case', 'mode', 'transport']))
        for metric in (['Ir', 'Bcm', 'Bim'] if row['mode'] == 'callgrind' else ['blocks', 'bytes']):
            before, after = control['counts'][metric], row['counts'][metric]
            change = f'{100*(after/before-1):+.3f}%' if before else 'n/a'
            lines.append(f"| {row['transport']} | {row['case']} | {metric} | {before:,.3f} | {after:,.3f} | {change} |")
    report = '\n'.join(lines) + '\n'
    (output / 'comparison.md').write_text(report)
    print(report)


if __name__ == '__main__':
    main()
