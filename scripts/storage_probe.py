#!/usr/bin/env python3
"""Run the prebuilt storage research probe serially in fresh processes."""
import argparse
import datetime
import hashlib
import json
from pathlib import Path
import platform
import random
import subprocess


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--base', type=Path, required=True, help='Existing directory on the filesystem to measure')
    parser.add_argument('--output', type=Path, required=True, help='New directory for evidence; must not exist')
    parser.add_argument('--binary', type=Path, default=Path('target/release/examples/storage_probe'))
    parser.add_argument('--trials', type=int, default=3)
    args = parser.parse_args()
    if args.trials < 1:
        parser.error('--trials must be positive')
    root = Path(__file__).resolve().parents[1]
    binary = args.binary.resolve(strict=True)
    base = args.base.resolve(strict=True)
    args.output.mkdir(parents=True, exist_ok=False)
    sources = ['examples/storage_probe.rs', 'scripts/storage_probe.py', 'src/storage.rs', 'src/storage/components.rs',
               'src/storage/history.rs', 'src/storage/io.rs', 'src/orm.rs', 'src/orm/components.rs', 'Cargo.lock']
    mount = subprocess.check_output(['findmnt', '-J', '-T', str(base)], text=True)
    environment = {
        'started_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
        'platform': platform.platform(), 'filesystem': json.loads(mount),
        'binary_sha256': digest(binary), 'sources_sha256': {name: digest(root / name) for name in sources},
        'git_head': subprocess.check_output(['git', '-C', str(root), 'rev-parse', 'HEAD'], text=True).strip(),
        'rustc': subprocess.check_output(['rustc', '--version'], text=True).strip(),
        'trials': args.trials, 'order_seed': 20261001,
        'notes': 'Uncontrolled shared host; warm-cache reads/reopen; no timing gates; logical file bytes, not device writes. Git HEAD alone does not identify the dirty source tree; source hashes are included.',
    }
    cpuinfo = Path('/proc/cpuinfo')
    if cpuinfo.exists():
        environment['cpu_model'] = next(line.split(':', 1)[1].strip() for line in cpuinfo.read_text().splitlines() if line.startswith('model name'))
    governor = Path('/sys/devices/system/cpu/cpu0/cpufreq/scaling_governor')
    if governor.exists():
        environment['cpu0_governor'] = governor.read_text().strip()
    (args.output / 'environment.json').write_text(json.dumps(environment, indent=2) + '\n')
    cases = [('whole', 2, 128, False), ('whole', 2, 128, True),
             ('components', 2, 128, False), ('components', 2, 128, True),
             ('components', 16, 128, False), ('components', 64, 128, False), ('components', 256, 128, False),
             ('direct', 2, 256, False), ('worker', 2, 256, False),
             ('batch', 1, 256, False), ('batch', 4, 256, False), ('batch', 16, 256, False)]
    rng = random.Random(environment['order_seed'])
    with (args.output / 'runs.jsonl').open('w') as output:
        for trial in range(args.trials):
            order = list(cases)
            rng.shuffle(order)
            for mode, size, writes, hold in order:
                command = [str(binary), str(base), mode, str(size), str(writes), str(hold).lower()]
                run = subprocess.run(command, check=True, capture_output=True, text=True, timeout=180)
                result = json.loads(run.stdout)
                result['trial'] = trial + 1
                result['command'] = command
                output.write(json.dumps(result) + '\n')
                output.flush()
                print(f'trial {trial + 1}: {mode} size={size} hold={hold}', flush=True)


if __name__ == '__main__':
    main()
