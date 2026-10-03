#!/usr/bin/env python3
"""Measure concurrency candidates serially, or recompute a frozen summary."""
import argparse
from collections import defaultdict
import datetime
import hashlib
import json
from pathlib import Path
import platform
import random
import resource
import statistics
import subprocess


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def summarize(directory):
    groups = defaultdict(list)
    for line in (directory / 'runs.jsonl').read_text().splitlines():
        row = json.loads(line)
        assert row['validated'] and row['operations'] > 0
        if row['kind'] == 'worker':
            assert row['verified_reopen'] and row['rejected'] == 0
        groups[(row['kind'], row['variant'], row['threads'], row['batch'])].append(row)
    fields = ['operations_per_second', 'ns_per_operation', 'elapsed_ns',
              'latency_p50_ns', 'latency_p99_ns', 'latency_max_ns',
              'full_retries', 'empty_polls', 'publications', 'process_cpu_seconds']
    scenarios = []
    for (kind, variant, threads, batch), rows in sorted(groups.items()):
        item = dict(kind=kind, variant=variant, threads=threads, batch=batch,
                    trials=len(rows), metrics={})
        for field in fields:
            if field in rows[0]:
                values = [row[field] for row in rows]
                item['metrics'][field] = dict(median=statistics.median(values),
                                             min=min(values), max=max(values))
        scenarios.append(item)
    (directory / 'summary.json').write_text(json.dumps({
        'aggregation': 'Median and range of per-run statistics, never pooled percentiles. '
                       'Snapshot cases have no latency samples (zero fields mean not measured). '
                       'Process CPU includes setup and verification; elapsed_ns is the timed region.',
        'scenarios': scenarios,
    }, indent=2) + '\n')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--base', type=Path, help='Existing directory on the measured filesystem')
    parser.add_argument('--output', type=Path, help='New evidence directory; must not exist')
    parser.add_argument('--binary', type=Path,
                        default=Path('target/concurrency-research/release/capntproto-concurrency-probe'))
    parser.add_argument('--trials', type=int, default=3)
    parser.add_argument('--summarize', type=Path)
    args = parser.parse_args()
    if args.summarize:
        summarize(args.summarize)
        return
    if not args.base or not args.output or args.trials < 1:
        parser.error('--base, --output and positive --trials are required')
    root = Path(__file__).resolve().parents[1]
    binary = args.binary.resolve(strict=True)
    base = args.base.resolve(strict=True)
    if not base.is_dir():
        parser.error('--base must be a directory')
    args.output.mkdir(parents=True, exist_ok=False)
    sources = ['scripts/concurrency_probe.py', 'Cargo.toml', 'Cargo.lock',
               'benchmarks/concurrency/Cargo.toml', 'benchmarks/concurrency/Cargo.lock',
               'src/storage.rs', 'src/storage/components.rs', 'src/storage/history.rs',
               'src/storage/io.rs', 'src/storage/worker.rs', 'src/storage/worker/operations.rs']
    sources.extend(str(p.relative_to(root)) for p in sorted((root / 'benchmarks/concurrency/src').glob('*.rs')))
    environment = {
        'started_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
        'platform': platform.platform(),
        'filesystem': json.loads(subprocess.check_output(['findmnt', '-J', '-T', str(base)], text=True)),
        'binary_sha256': digest(binary),
        'sources_sha256': {name: digest(root / name) for name in sources},
        'git_head': subprocess.check_output(['git', '-C', str(root), 'rev-parse', 'HEAD'], text=True).strip(),
        'rustc': subprocess.check_output(['rustc', '--version'], text=True).strip(),
        'trials': args.trials, 'order_seed': 20261002,
        'notes': 'Uncontrolled shared host. Each trial shuffles 28 scenarios run serially in fresh processes. '
                 'Queue probes yield on full/empty; this is saturation, not production parking or admission. '
                 'Snapshot writers sleep 100us between publications; achieved counts vary by duration. '
                 'Worker cases use one outstanding request per producer, default budgets, real fsync for writes. '
                 'No disk errors, CPU pinning, warmup exclusion or performance gates. '
                 'Git HEAD alone does not identify the dirty checkout; source and lockfile hashes are included.',
    }
    cpuinfo = Path('/proc/cpuinfo')
    if cpuinfo.exists():
        environment['cpu_model'] = next(line.split(':', 1)[1].strip() for line in cpuinfo.read_text().splitlines() if line.startswith('model name'))
    governor = Path('/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor')
    if governor.exists():
        environment['cpu0_governor'] = governor.read_text().strip()
    (args.output / 'environment.json').write_text(json.dumps(environment, indent=2) + '\n')
    cases = [('queue', v, p, b) for v in ['std', 'parking', 'array', 'tokio']
             for p in [1, 8] for b in [1, 16]]
    cases += [('snapshot', v, p, 1) for v in ['mutex', 'rwlock', 'guard', 'owned'] for p in [1, 8]]
    cases += [('worker', v, p, 1) for v in ['status', 'write'] for p in [1, 8]]
    rng = random.Random(environment['order_seed'])
    with (args.output / 'runs.jsonl').open('w') as output:
        for trial in range(args.trials):
            order = list(cases)
            rng.shuffle(order)
            for kind, variant, threads, batch in order:
                command = [str(binary), str(base), kind, variant, str(threads), str(batch)]
                before = resource.getrusage(resource.RUSAGE_CHILDREN)
                run = subprocess.run(command, check=True, capture_output=True, text=True, timeout=180)
                after = resource.getrusage(resource.RUSAGE_CHILDREN)
                row = json.loads(run.stdout)
                row.update(trial=trial + 1, command=command,
                           process_cpu_seconds=after.ru_utime + after.ru_stime - before.ru_utime - before.ru_stime)
                output.write(json.dumps(row) + '\n')
                output.flush()
                print(f'trial {trial + 1}: {kind}/{variant}/{threads}/{batch}', flush=True)
    summarize(args.output)


if __name__ == '__main__':
    main()
