#!/usr/bin/env python3
"""Post-timing encrypted diagnostics. Compare marginal work, never instrumented latency."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
import importlib.util
spec = importlib.util.spec_from_file_location('secure_runner', Path(__file__).with_name('secure-runner.py'))
r = importlib.util.module_from_spec(spec)
spec.loader.exec_module(r)


class Diagnostic(r.Campaign):
    mode = 'callgrind'

    def launch(self, label, argv, transport, directory, role, cpu, overrides=None):
        env = dict(os.environ, CAPNTPROTO_BENCH_CERTS=str(self.certs),
                   CAPNTPROTO_BENCH_TRANSPORT=transport,
                   CAPNTPROTO_BENCH_PORTABLE='1' if r.engine(label) == 'portable' else '0',
                   CAPNTPROTO_BENCH_SERVER_NAME='localhost')
        env.update(overrides or {})
        command = ['taskset', '-c', str(cpu), 'valgrind', '--tool=' + self.mode]
        if self.mode == 'callgrind':
            command += ['--cache-sim=yes', '--branch-sim=yes',
                        '--callgrind-out-file=' + str(directory / (role + '.callgrind'))]
        else:
            command += ['--num-callers=24', '--dhat-out-file=' + str(directory / (role + '.dhat.json'))]
        command += [str(self.binaries[label]), *argv]
        (directory / (role + '.command.json')).write_text(json.dumps(command) + '\n')
        with (directory / (role + '.stderr')).open('x') as error:
            return subprocess.Popen(command, env=env, stdout=subprocess.PIPE, stderr=error,
                                    text=True, start_new_session=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--rust', type=Path, required=True)
    parser.add_argument('--cpp', type=Path, required=True)
    parser.add_argument('--portable-baseline', type=Path)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    r.idle_host()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    binaries = dict(legacy=args.rust.resolve(strict=True), portable=args.rust.resolve(strict=True),
                    cpp=args.cpp.resolve(strict=True))
    if args.portable_baseline:
        binaries['portable-baseline'] = args.portable_baseline.resolve(strict=True)
    certs = r.certificates.generate(output / 'certificates')
    cpus, cpu_topology = r.physical_cpus()
    campaign = Diagnostic(binaries, output, certs, cpus)
    scope = dict(diagnostic=True, warmup=10, points=[50, 500], cpu_topology=cpu_topology, cpus=cpus,
                 binaries={k: dict(path=str(v), sha256=r.topology.digest(v)) for k, v in binaries.items()},
                 metric='Both peers: (500-operation profile minus 50-operation profile) / 450. '
                        'Callgrind instructions/cache/branches are simulated. DHAT reports cumulative allocations.',
                 trials=[])
    def save():
        (output / 'scope.json').write_text(json.dumps(scope, indent=2) + '\n')
    save()
    cases = {c['name']: c for c in r.catalog()}
    for mode in ['callgrind', 'dhat']:
        campaign.mode = mode
        for transport, name in [('tls', 'dependent-read'), ('tls', 'pipelined-read'), ('quic', 'pipelined-read')]:
            for label in binaries:
                if not r.supported(label, transport, cases[name]):
                    continue
                for iterations in [50, 500]:
                    prefix = f'{mode}-{transport}-{name}-{label}-{iterations}'
                    result = campaign.trial(cases[name], label, transport, prefix, 10, iterations)
                    scope['trials'].append(dict(path=prefix, mode=mode, transport=transport,
                                               case=name, implementation=label, iterations=iterations,
                                               services=result['services']))
                    save()
                    print(prefix, flush=True)


if __name__ == '__main__':
    main()
