#!/usr/bin/env python3
"""Bounded AFL++ campaigns; findings fail CI even when afl-fuzz exits zero."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import signal
import subprocess
import time

TARGETS = ('capnp_framing', 'capnp_pointers', 'capnp_schema', 'rpc_lifecycle')


def statistics(path):
    fields = {}
    for line in Path(path).read_text().splitlines():
        key, separator, value = line.partition(':')
        if separator:
            fields[key.strip()] = value.strip()
    result = {name: int(fields[name]) for name in
              ('execs_done', 'corpus_count', 'saved_crashes', 'saved_hangs', 'edges_found')}
    if any(v < 0 for v in result.values()) or not result['execs_done'] or not result['edges_found']:
        raise ValueError('AFL produced no instrumented executions or invalid counters')
    for name in ('stability', 'bitmap_cvg'):
        result[name] = float(fields[name].rstrip('%'))
        if not 0 <= result[name] <= 100:
            raise ValueError('invalid AFL percentage')
    return result


def execute(command, log, env, timeout):
    with log.open('wb') as output:
        process = subprocess.Popen(command, stdout=output, stderr=subprocess.STDOUT,
                                   env=env, start_new_session=True)
        try:
            return process.wait(timeout=timeout)
        finally:
            # No fuzzer/forkserver may outlive a timed-out or cancelled campaign.
            try:
                os.killpg(process.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            process.wait()


def run(output, binaries, corpus, seconds):
    output.mkdir(parents=True, exist_ok=True)
    report_path = output / 'afl.json'
    report = {'format': 1, 'engine': 'afl++', 'cargo_afl': '0.18.2',
              'afl_version': '4.40c', 'cmplog': False,
              'seconds_per_target': seconds, 'campaigns': [], 'passed': False}
    report_path.write_text(json.dumps(report, indent=2))
    version = subprocess.check_output(['cargo', 'afl', '--version'], text=True).strip()
    if version != 'cargo-afl 0.18.2 (AFL++ version 4.40c)':
        raise ValueError(f'unexpected AFL tool: {version}')
    env = dict(os.environ, AFL_NO_UI='1', AFL_SKIP_CPUFREQ='1', AFL_NO_AFFINITY='1',
               AFL_FUZZER_LOOPCOUNT='1000')
    for target in TARGETS:
        campaign = dict(target=target, passed=False, statistics=None, error=None)
        report['campaigns'].append(campaign)
        log = output / f'afl-{target}.log'
        findings = output / target
        # Require a fresh output directory. Never reuse old crashes or counters.
        if findings.exists():
            raise ValueError(f'campaign output already exists: {findings}')
        command = ['cargo', 'afl', 'fuzz', '-i', str(corpus / target), '-o', str(findings),
                   '-V', str(seconds), '-s', '1', '-G', '4096', '-t', '1000',
                   '-m', 'none', '-c', '-', '--', str(binaries / f'afl_{target}')]
        campaign['command'] = command
        started = time.monotonic()
        print(f'AFL++ {target}: {seconds}s mutation budget', flush=True)
        try:
            code = execute(command, log, env, seconds + 300)
            stats = statistics(findings / 'default/fuzzer_stats')
            campaign['statistics'] = stats
            campaign['exit_code'] = code
            # IJON is linked by the annotation calls and must be detected by AFL.
            if target == 'rpc_lifecycle' and 'Using IJON feature.' not in log.read_text(errors='replace'):
                raise ValueError('AFL did not report IJON support for the RPC target')
            if code or stats['saved_crashes'] or stats['saved_hangs']:
                raise ValueError('AFL failed or saved crash/hang inputs; inspect the retained corpus')
            campaign['passed'] = True
        except (OSError, ValueError, subprocess.TimeoutExpired) as error:
            campaign['error'] = str(error)
        finally:
            campaign['seconds'] = time.monotonic() - started
            if log.exists():
                campaign['log_sha256'] = hashlib.sha256(log.read_bytes()).hexdigest()
            report_path.write_text(json.dumps(report, indent=2) + '\n')
        print(f'AFL++ {target}: {campaign["statistics"] or campaign["error"]}', flush=True)
    report['passed'] = all(c['passed'] for c in report['campaigns'])
    report_path.write_text(json.dumps(report, indent=2) + '\n')
    return report['passed']


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--binaries', type=Path, required=True)
    parser.add_argument('--corpus', type=Path, required=True)
    parser.add_argument('--seconds', type=int, default=120)
    args = parser.parse_args()
    if not 1 <= args.seconds <= 3600:
        parser.error('--seconds must be between 1 and 3600')
    raise SystemExit(0 if run(args.output.resolve(), args.binaries.resolve(),
                             args.corpus.resolve(), args.seconds) else 1)
