#!/usr/bin/env python3
"""Matched independent-process service chains; raw trials are never overwritten."""
import argparse
import collections
import gzip
import hashlib
import json
import os
from pathlib import Path
import platform
import selectors
import signal
import struct
import subprocess
import time

ROOT = Path(__file__).resolve().parents[2]
EVENT = struct.Struct('<QQQQqiIII')


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def percentile(values, p):
    return sorted(values)[(len(values) * p + 99) // 100 - 1]


def host_state():
    """Outside the timed workload; retain quota/throttling and CPU steal context."""
    state = {'monotonic_ns': time.monotonic_ns()}
    for name in ['/proc/stat', '/proc/loadavg', '/sys/fs/cgroup/cpu.max', '/sys/fs/cgroup/cpu.stat']:
        try:
            value = Path(name).read_text()
            if name == '/proc/stat':
                value = '\n'.join(row for row in value.splitlines() if row.startswith('cpu'))
            state[name] = value
        except OSError:
            state[name] = None
    return state


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--rust', type=Path, required=True)
    parser.add_argument('--cpp', type=Path, required=True)
    parser.add_argument('--baseline', type=Path, help='Optional frozen Rust control, interleaved with the candidate and C++.')
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--repetitions', type=int, default=4)
    parser.add_argument('--warmup', type=int)
    parser.add_argument('--iterations', type=int)
    parser.add_argument('--cases', default='')
    parser.add_argument('--instrument', choices=['none', 'socket', 'wire', 'callgrind'], default='none')
    parser.add_argument('--mixed', action='store_true', help='Also validate alternating Rust/C++ service chains in both directions.')
    args = parser.parse_args()
    diagnostic = args.instrument != 'none'
    warmup = args.warmup if args.warmup is not None else (2 if diagnostic else 100)
    iterations = args.iterations if args.iterations is not None else (10 if diagnostic else 1000)
    assert 0 <= warmup <= 100000 and 1 <= iterations <= 100000 and 1 <= args.repetitions <= 100
    binaries = {'legacy': args.rust.resolve(strict=True), 'cpp': args.cpp.resolve(strict=True)}
    if args.baseline is not None:
        binaries['baseline'] = args.baseline.resolve(strict=True)
    qualifying = not diagnostic and warmup >= 100 and iterations >= 1000 and args.repetitions >= 2 * len(binaries)
    catalog_path = ROOT / 'benchmarks/rpc/topologies.json'
    catalog = json.loads(catalog_path.read_text())
    selected = set(args.cases.split(',')) if args.cases else {c['name'] for c in catalog}
    assert selected <= {c['name'] for c in catalog}, 'unknown topology'
    cases = [c for c in catalog if c['name'] in selected]
    # Select physical cores, recording oversubscription for longer chains.
    topology = subprocess.check_output(['lscpu', '-b', '-p=CPU,CORE,SOCKET'], text=True)
    allowed = os.sched_getaffinity(0)
    cpus, cores = [], set()
    for row in topology.splitlines():
        if row.startswith('#') or not row:
            continue
        cpu, core, socket = map(int, row.split(','))
        if cpu in allowed and (core, socket) not in cores:
            cpus.append(cpu)
            cores.add((core, socket))
    assert len(cpus) >= 2, 'requires distinct client and server cores'
    for path in Path('/proc').glob('[0-9]*/comm'):
        try:
            name = path.read_text().strip()
            if name in ['rustc', 'clippy-driver', 'rustfmt', 'cargo', 'miri', 'rust-lld', 'java', 'cc1plus', 'clang', 'clang++', 'strace'] or 'callgrind' in name:
                assert (path.parent / 'stat').read_text().split(') ', 1)[1].split()[0] == 'Z', f'competing build/profiler: {path}'
        except (FileNotFoundError, ProcessLookupError):
            pass
    out = args.output.resolve()
    out.mkdir(parents=True, exist_ok=False)
    probe = out / 'probe.so'
    if args.instrument == 'socket':
        source = ROOT / 'dev/src/cloud/remote/timing/probe.c'
        (out / 'probe.c').write_bytes(source.read_bytes())
        subprocess.run(['cc', '-O2', '-shared', '-fPIC', '-o', str(probe), str(source), '-ldl'], check=True)
    scope = dict(topology_version=1, instrument=args.instrument, qualifying_budget=qualifying, warmup=warmup, iterations=iterations,
                 repetitions=args.repetitions, cases=cases, binaries={k: dict(path=str(v), sha256=digest(v)) for k, v in binaries.items()},
                 fixture_sha256=digest(catalog_path), runner_sha256=digest(__file__), cpu_topology=topology,
                 uname=list(platform.uname()), client_cpu=cpus[0], server_cpus=cpus[1:],
                 order_policy='Rotate variants, then reverse every complete cycle; two full cycles balance positions.',
                 metric='Whole concurrent batch latency, including construction, all service hops, validation, response destruction and ten-second deadline.',
                 goal='Every uninstrumented cell pooled p50 <= 0.70 * C++ pooled p50; diagnostic timings do not qualify.',
                 trials=[])
    (out / 'scope.json').write_text(json.dumps(scope, indent=2) + '\n')

    def launch(label, argv, prefix, cpu):
        env = dict(os.environ, CAPNTPROTO_BENCH_PORTABLE='0')
        command = ['taskset', '-c', str(cpu)]
        if args.instrument == 'socket':
            env.update(LD_PRELOAD=str(probe), CAPNTPROTO_IO_TRACE=str(prefix) + '.bin')
        elif args.instrument == 'wire':
            command += ['strace', '-f', '-qq', '-ttt', '-T', '-yy', '-xx', '-v', '-s', '8388608', '-e',
                        'trace=read,write,readv,writev,sendto,recvfrom,sendmsg,recvmsg,epoll_wait,epoll_pwait,getsockopt,setsockopt', '-o', str(prefix) + '.strace']
        elif args.instrument == 'callgrind':
            command += ['valgrind', '--tool=callgrind', '--collect-atstart=yes', '--callgrind-out-file=' + str(prefix) + '.callgrind']
        with Path(str(prefix) + '.stderr').open('x') as error:
            process = subprocess.Popen([*command, str(binaries[label]), *argv], env=env, stdout=subprocess.PIPE,
                                       stderr=error, text=True, start_new_session=True)
        return process

    def stop(process):
        if process.poll() is None:
            os.killpg(process.pid, signal.SIGTERM)
        try:
            process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            os.killpg(process.pid, signal.SIGKILL)
            process.wait(timeout=10)

    def trial(case, client_label, services, prefix, warm, count):
        directory = out / prefix
        directory.mkdir()
        host = {'scope': 'Entire process lifecycle, including setup and teardown; outside client latency samples.',
                'before': host_state()}
        workers = []
        address = None
        placement = []
        client = None
        try:
            for hop in reversed(range(case['hops'])):
                label = services[hop]
                cpu = cpus[1 + hop % (len(cpus) - 1)]
                command = ['topology-server', '127.0.0.1:0'] + ([address] if address else [])
                process = launch(label, command, directory / f'service-{hop}', cpu)
                workers.append(process)
                with selectors.DefaultSelector() as selector:
                    selector.register(process.stdout, selectors.EVENT_READ)
                    assert selector.select(30), 'server readiness timeout'
                    line = process.stdout.readline()
                (directory / f'service-{hop}.ready.json').write_text(line)
                ready = json.loads(line)
                assert ready['hops'] == case['hops'] - hop, 'incorrect chain length'
                address = ready.get('address') or f"127.0.0.1:{ready['port']}"
                placement.append(dict(hop=hop, implementation=label, cpu=cpu, address=address))
            client = launch(client_label, ['topology-measure', address, case['name'], str(warm), str(count)], directory / 'client', cpus[0])
            output, _ = client.communicate(timeout=300)
            (directory / 'trial.json').write_text(output)
            assert client.returncode == 0, f'client failed: {directory}'
            data = json.loads(output)
            assert data['topology_version'] == 1 and data['case'] == case
            assert data['implementation'] == ('capnp-cpp' if client_label == 'cpp' else 'legacy')
            assert data['iterations'] == len(data['latency_ns']) == count and data['warmup'] == warm
            assert all(type(n) is int and n > 0 for n in data['latency_ns'])
            assert data['elapsed_ns'] >= sum(data['latency_ns'])
            assert all(p.poll() is None for p in workers), 'service exited during trial'
            for process in workers:
                (directory / f'process-{process.pid}.stat').write_text(Path(f'/proc/{process.pid}/stat').read_text())
            return data, placement
        finally:
            if client is not None:
                stop(client)
            for process in reversed(workers):
                stop(process)
            host['after'] = host_state()
            (directory / 'host.json').write_text(json.dumps(host, indent=2) + '\n')
            for trace in directory.glob('*.strace'):
                with trace.open('rb') as source, gzip.open(str(trace) + '.gz', 'wb', compresslevel=1) as sink:
                    import shutil
                    shutil.copyfileobj(source, sink)
                trace.unlink()

    if args.mixed:
        for case in cases:
            for index, label in enumerate(['legacy', 'cpp']):
                services = [['legacy', 'cpp'][(hop + index + 1) % 2] for hop in range(case['hops'])]
                prefix = f"mixed-{case['name']}-{label}"
                _, placement = trial(case, label, services, prefix, 2, 5)
                scope['trials'].append(dict(path=prefix, case=case['name'], client=label, services=placement, mixed=True))
                (out / 'scope.json').write_text(json.dumps(scope, indent=2) + '\n')
    for rep in range(args.repetitions):
        order = list(binaries)
        order = order[rep % len(binaries):] + order[:rep % len(binaries)]
        if rep // len(binaries) % 2:
            order.reverse()
        for case in cases:
            for label in order:
                prefix = f"{rep}-{case['name']}-{label}"
                _, placement = trial(case, label, [label] * case['hops'], prefix, warmup, iterations)
                scope['trials'].append(dict(path=prefix, case=case['name'], client=label, services=placement, mixed=False, repetition=rep))
                (out / 'scope.json').write_text(json.dumps(scope, indent=2) + '\n')
            print(rep, case['name'], flush=True)
    rows = []
    for case in cases:
        cells = {}
        for label in binaries:
            trials = [json.loads((out / f"{rep}-{case['name']}-{label}" / 'trial.json').read_text()) for rep in range(args.repetitions)]
            values = [n for trial in trials for n in trial['latency_ns']]
            rate = len(values) * 1e9 / sum(t['elapsed_ns'] for t in trials)
            cells[label] = dict(samples=len(values), **{f'p{p}_ns': percentile(values, p) for p in [50, 95, 99]},
                                repetition_p50_ns=[percentile(t['latency_ns'], 50) for t in trials],
                                batches_per_second=rate, logical_requests_per_second=rate * case['width'],
                                service_calls_per_second=rate * case['width'] * case['hops'])
        ratio = cells['legacy']['p50_ns'] / cells['cpp']['p50_ns']
        rows.append(dict(case=case, variants=cells, legacy_over_cpp=ratio, meets_target=ratio <= 0.70 if qualifying else None))
    journals = []
    for path in sorted(out.glob('*/*.bin')):
        raw = path.read_bytes()
        count, = struct.unpack_from('<Q', raw)
        assert count <= 131072 and len(raw) == 8 + count * EVENT.size, f'incomplete probe: {path}'
        counts = collections.Counter()
        for _, _, _, _, result, _, op, _, _ in EVENT.iter_unpack(raw[8:]):
            direction = 'read' if op in [1, 3, 5, 7, 9] else ('write' if op in [2, 4, 6, 8, 10] else 'epoll')
            counts[direction + ('_positive' if result > 0 else '_nonpositive')] += 1
        journals.append(dict(path=str(path.relative_to(out)), counts=dict(counts), sha256=digest(path)))
    summary = dict(rows=rows, socket_journals=journals, diagnostic=diagnostic, qualifying_budget=qualifying,
                   all_cells_meet_target=all(r['meets_target'] for r in rows) if qualifying else None)
    (out / 'summary.json').write_text(json.dumps(summary, indent=2) + '\n')
    print(json.dumps(rows, indent=2))


if __name__ == '__main__':
    main()
