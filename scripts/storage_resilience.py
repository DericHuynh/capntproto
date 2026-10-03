#!/usr/bin/env python3
"""Run fixed-rate storage experiments serially, or recompute their summary."""
import argparse
from collections import defaultdict
import datetime
import hashlib
import json
from pathlib import Path
import platform
import random
import statistics
import subprocess


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def summarize(directory):
    groups = defaultdict(list)
    for line in (directory / 'runs.jsonl').read_text().splitlines():
        row = json.loads(line)
        assert row['verified_reopen']
        if row['scenario'] != 'lost-reply':
            assert row['offered'] == row['admitted'] + row['rejected_count'] + row['rejected_bytes']
            assert row['admitted'] == row['committed'] + row['expired_before_execution']
            assert row['peak_outstanding_count'] <= row['count_limit']
            assert row['peak_outstanding_payload_bytes'] <= row['payload_byte_limit']
        groups[row['scenario']].append(row)
    summary = []
    fields = ['committed', 'admitted', 'rejected_count', 'rejected_bytes', 'rejected_large',
              'committed_large', 'expired_before_execution', 'committed_after_deadline',
              'peak_outstanding_count', 'peak_outstanding_payload_bytes',
              'pause_us', 'drain_after_arrival_window_us', 'total_elapsed_us',
              'generator_lateness.p99_us', 'generator_lateness.max_us',
              'success_latency.p50_us', 'success_latency.p99_us',
              'queue_age_at_dequeue.p99_us', 'commit_service.p50_us', 'commit_service.p99_us']
    for name, rows in sorted(groups.items()):
        item = {'scenario': name, 'trials': len(rows), 'metrics': {}}
        for field in ['committed'] if name == 'lost-reply' else fields:
            values = []
            for row in rows:
                value = row
                for key in field.split('.'):
                    value = value[key]
                values.append(value)
            item['metrics'][field] = {'median': statistics.median(values), 'min': min(values), 'max': max(values)}
        summary.append(item)
    (directory / 'summary.json').write_text(json.dumps({
        'aggregation': 'Median and range of per-run statistics, never pooled percentiles. Rejections and expiries are separate outcomes, not successful latencies.',
        'scenarios': summary,
    }, indent=2) + '\n')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--base', type=Path, help='Existing directory on the filesystem to measure')
    parser.add_argument('--output', type=Path, help='New evidence directory; must not exist')
    parser.add_argument('--binary', type=Path, default=Path('target/release/examples/storage_resilience'))
    parser.add_argument('--trials', type=int, default=3)
    parser.add_argument('--summarize', type=Path, help='Only recompute summary.json in this evidence directory')
    args = parser.parse_args()
    if args.summarize:
        summarize(args.summarize)
        return
    if not args.base or not args.output or args.trials < 1:
        parser.error('--base, --output, and a positive --trials are required for measurement')
    root = Path(__file__).resolve().parents[1]
    binary = args.binary.resolve(strict=True)
    base = args.base.resolve(strict=True)
    if not base.is_dir():
        parser.error('--base must be a directory')
    args.output.mkdir(parents=True, exist_ok=False)
    sources = ['examples/storage_resilience.rs', 'scripts/storage_resilience.py',
               'src/storage.rs', 'src/storage/components.rs', 'src/storage/components/crash_tests.rs',
               'src/storage/history.rs', 'src/storage/io.rs', 'Cargo.toml', 'Cargo.lock']
    environment = {
        'started_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
        'platform': platform.platform(),
        'filesystem': json.loads(subprocess.check_output(['findmnt', '-J', '-T', str(base)], text=True)),
        'binary_sha256': digest(binary), 'sources_sha256': {name: digest(root / name) for name in sources},
        'git_head': subprocess.check_output(['git', '-C', str(root), 'rev-parse', 'HEAD'], text=True).strip(),
        'rustc': subprocess.check_output(['rustc', '--version'], text=True).strip(),
        'trials': args.trials, 'order_seed': 20261002,
        'notes': 'Uncontrolled shared host. One-second fixed-rate arrival window, no client retries. Real fsync per commit; one injected worker sleep, not disk EIO. Byte credits account for queued/executing payloads, not RSS or history/index bytes. Git HEAD alone does not identify this dirty checkout; source hashes are included.',
    }
    cpuinfo = Path('/proc/cpuinfo')
    if cpuinfo.exists():
        environment['cpu_model'] = next(line.split(':', 1)[1].strip() for line in cpuinfo.read_text().splitlines() if line.startswith('model name'))
    governor = Path('/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor')
    if governor.exists():
        environment['cpu0_governor'] = governor.read_text().strip()
    (args.output / 'environment.json').write_text(json.dumps(environment, indent=2) + '\n')
    cases = ['baseline', 'steady-stall', 'overload-8', 'overload-64', 'overload-256',
             'deadline-25ms', 'mixed-4mib', 'mixed-128kib', 'lost-reply']
    rng = random.Random(environment['order_seed'])
    with (args.output / 'runs.jsonl').open('w') as output:
        for trial in range(args.trials):
            order = list(cases)
            rng.shuffle(order)
            for case in order:
                command = [str(binary), str(base), case]
                run = subprocess.run(command, check=True, capture_output=True, text=True, timeout=180)
                row = json.loads(run.stdout)
                row.update(trial=trial + 1, command=command)
                output.write(json.dumps(row) + '\n')
                output.flush()
                print(f'trial {trial + 1}: {case}', flush=True)
    summarize(args.output)


if __name__ == '__main__':
    main()
