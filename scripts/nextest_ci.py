#!/usr/bin/env python3
"""Run nextest in CI with a distinct JUnit artifact for each invocation."""
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import tempfile


def run(args):
    # Everything before -- is the command prefix (e.g. cargo +nightly careful).
    split = args.index('--')
    prefix, selection = args[:split], args[split + 1:]
    root = Path(__file__).resolve().parent.parent
    reports = root / 'target/nextest/reports'
    reports.mkdir(parents=True, exist_ok=True)
    name = hashlib.sha256('\0'.join(args).encode()).hexdigest()[:16]
    report = reports / f'{name}.xml'
    report.unlink(missing_ok=True)
    config = (root / '.config/nextest.toml').read_text()
    config += '\n[profile.evidence]\ninherits = "ci"\n[profile.evidence.junit]\npath = ' + json.dumps(str(report)) + '\n'
    with tempfile.TemporaryDirectory(prefix='nextest-ci-') as directory:
        config_path = Path(directory) / 'nextest.toml'
        config_path.write_text(config)
        command = prefix + ['nextest', 'run', '--config-file', str(config_path), '--profile', 'evidence'] + selection
        (reports / f'{name}.command.txt').write_text(' '.join(command) + '\n')
        code = subprocess.call(command, cwd=root)
    if code == 0 and not report.is_file():
        raise RuntimeError('successful nextest run produced no JUnit report')
    return code


if __name__ == '__main__':
    sys.exit(run(sys.argv[1:]))
