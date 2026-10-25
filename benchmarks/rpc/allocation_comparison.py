"""Compare saved all-peer allocation counts; never execute benchmark workloads."""
import argparse
import collections
import json
import math
from pathlib import Path


def compare(summary, scope):
    trials = collections.defaultdict(list)
    peers = collections.defaultdict(list)
    totals = {}
    for row in scope['trials']:
        if row['mode'] == 'dhat':
            trials[row['transport'], row['case'], row['implementation']].append(row)
    for row in summary['profile_peers']:
        if row['mode'] == 'dhat':
            peers[row['transport'], row['case'], row['variant']].append(row)
    for row in summary['profiles']:
        if row['mode'] == 'dhat':
            key = row['transport'], row['case'], row['variant']
            if key in totals:
                raise ValueError(f'duplicate allocation total: {key}')
            totals[key] = row['per_operation']
    workloads = scope.get('allocation_workloads', scope.get('workloads'))
    if not workloads:
        raise ValueError('allocation workload scope is required')

    def cell(transport, case, variant, points):
        key = transport, case, variant
        pair = trials[key]
        if len(pair) != 2 or sorted(r['iterations'] for r in pair) != points:
            return None
        expected = [{'client', *[f"service-{s['hop']}" for s in r['services']]} for r in pair]
        if expected[0] != expected[1] or any(r.get('points', [50, 500]) != points for r in pair):
            return None
        rows = peers[key]
        if len(rows) != len(expected[0]) or {r['peer'] for r in rows} != expected[0]:
            return None
        if any(r.get('points', [50, 500]) != points for r in rows):
            return None
        total = totals.get(key)
        if total is None:
            return None
        for metric in ['blocks', 'bytes']:
            count = sum(r['per_operation'][metric] for r in rows)
            if not math.isfinite(count) or count < 0:
                return None
            if not math.isclose(count, total[metric], rel_tol=1e-12, abs_tol=1e-9):
                raise ValueError(f'partial or inconsistent all-peer total: {key} {metric}')
        return dict(total)

    rows = []
    seen = set()
    for workload in workloads:
        transport, case, points = (workload[k] for k in ['transport', 'case', 'points'])
        if (transport, case) in seen:
            raise ValueError('duplicate workload')
        seen.add((transport, case))
        references = ['portable', 'cpp'] if transport == 'tls' else ['portable']
        variants = {v: cell(transport, case, v, points) for v in ['legacy', 'legacy-baseline', *references]}
        missing = [v for v in ['legacy', *references] if variants[v] is None]
        row = dict(transport=transport, case=case, points=points, variants=variants,
                   missing=missing, status='unmeasured', target_variant=None,
                   target_blocks=None, excess_blocks=None)
        if not missing:
            target = min(references, key=lambda v: variants[v]['blocks'])
            blocks = variants['legacy']['blocks']
            target_blocks = variants[target]['blocks']
            row.update(target_variant=target, target_blocks=target_blocks,
                       excess_blocks=blocks-target_blocks,
                       status='met' if blocks <= target_blocks or math.isclose(blocks, target_blocks, rel_tol=0, abs_tol=1e-9) else 'gap')
        rows.append(row)
    counts = collections.Counter(row['status'] for row in rows)
    return dict(source=summary['run'], metric='Cumulative allocation blocks per complete operation, summed across every peer.',
                expected_cases=len(rows), met=counts['met'], gaps=counts['gap'], unmeasured=counts['unmeasured'],
                allocation_target_met=bool(rows) and counts['met'] == len(rows),
                measurement_outcome=summary.get('measurement_outcome'),
                profile_failures=summary.get('profile_failures', []), rows=rows)


def markdown(result):
    lines = ['# Allocation comparison', '', result['metric'],
             'Short/long DHAT subtraction; concurrent cases count a complete batch. C++ QUIC is unsupported.',
             'A missing peer, trial, or required comparator leaves the target unmeasured. Values below are rounded only for display.', '',
             f"Target met in {result['met']}/{result['expected_cases']} cases; {result['gaps']} gaps; {result['unmeasured']} unmeasured.", '',
             '| Transport | Workload | Frozen legacy | Legacy | Portable | C++ | Target | Excess | Status |',
             '| --- | --- | ---: | ---: | ---: | ---: | --- | ---: | --- |']
    for row in result['rows']:
        variants = row['variants']
        values = ['—' if variants.get(v) is None else f"{variants[v]['blocks']:.2f}" for v in ['legacy-baseline', 'legacy', 'portable', 'cpp']]
        excess = '—' if row['excess_blocks'] is None else f"{row['excess_blocks']:+.2f}"
        lines.append('| ' + ' | '.join([row['transport'], row['case'], *values, row['target_variant'] or '—', excess, row['status']]) + ' |')
    lines += ['', 'Allocation counts do not establish latency or peak-memory improvements.']
    return '\n'.join(lines) + '\n'


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('summary', type=Path)
    parser.add_argument('profile_scope', type=Path)
    parser.add_argument('output', type=Path)
    args = parser.parse_args()
    result = compare(json.loads(args.summary.read_text()), json.loads(args.profile_scope.read_text()))
    args.output.mkdir(parents=True, exist_ok=True)
    (args.output/'allocation-comparison.json').write_text(json.dumps(result, indent=2)+'\n')
    (args.output/'allocation-comparison.md').write_text(markdown(result))
    print(f"Allocation target: {result['met']}/{result['expected_cases']} met, {result['gaps']} gaps, {result['unmeasured']} unmeasured")


if __name__ == '__main__':
    main()
