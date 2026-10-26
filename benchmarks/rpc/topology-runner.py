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
from latency_stats import summarize as latency_statistics

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
    parser.add_argument('--portable', type=Path, help='Portable driver binary; defaults to --rust with portable selected on every peer.')
    parser.add_argument('--cpp', type=Path, help='Required for TLS; unsupported for QUIC.')
    parser.add_argument('--transport', choices=['tls', 'quic'], default='tls')
    parser.add_argument('--comparators', action='store_true', help='Include realistic gRPC/HTTP2 and multiplexed binary Protobuf WebSocket services (TLS only).')
    parser.add_argument('--baseline', type=Path, help='Optional frozen Rust control, interleaved with the candidate and C++.')
    parser.add_argument('--portable-baseline', type=Path, help='Optional frozen portable control, explicitly selected on every peer.')
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--repetitions', type=int, default=5, help='Independent process repetitions per cell (default: 5).')
    parser.add_argument('--warmup', type=int)
    parser.add_argument('--iterations', type=int)
    parser.add_argument('--cases', default='')
    parser.add_argument('--instrument', choices=['none', 'socket', 'wire', 'callgrind'], default='none')
    parser.add_argument('--mixed', action='store_true', help='Also validate every ordered pair of legacy, portable and C++ in alternating service chains.')
    args = parser.parse_args()
    certificates = Path(os.environ.get('CAPNTPROTO_BENCH_CERTS', args.rust.resolve().parent / 'certificates')).resolve(strict=True)
    assert args.transport == 'quic' or args.cpp, '--cpp required for TLS comparison'
    diagnostic = args.instrument != 'none'
    warmup = args.warmup if args.warmup is not None else (2 if diagnostic else 100)
    iterations = args.iterations if args.iterations is not None else (10 if diagnostic else 1000)
    binaries = {'legacy': args.rust.resolve(strict=True),
                'portable': (args.portable or args.rust).resolve(strict=True)}
    if args.transport == 'tls':
        binaries['cpp'] = args.cpp.resolve(strict=True)
    if args.comparators:
        assert args.transport == 'tls', 'gRPC and WebSocket topologies require TLS, not the QUIC path'
        binaries.update(grpc=args.rust.resolve(strict=True), websocket=args.rust.resolve(strict=True))
    if args.baseline is not None:
        binaries['baseline'] = args.baseline.resolve(strict=True)
    if args.portable_baseline is not None:
        binaries['portable-baseline'] = args.portable_baseline.resolve(strict=True)
    assert 0 <= warmup <= 100000 and 1 <= iterations <= 100000 and 1 <= args.repetitions <= 100
    qualifying = (not diagnostic and warmup >= 100 and iterations >= 1000
                  and args.repetitions >= 5)
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
    scope = dict(transport=args.transport, cpp_quic='unsupported', topology_version=1, instrument=args.instrument, qualifying_budget=qualifying, warmup=warmup, iterations=iterations,
                 repetitions=args.repetitions, cases=cases, binaries={k: dict(path=str(v), sha256=digest(v)) for k, v in binaries.items()},
                 fixture_sha256=digest(catalog_path), runner_sha256=digest(__file__), cpu_topology=topology,
                 uname=list(platform.uname()), client_cpu=cpus[0], server_cpus=cpus[1:],
                 order_policy='Rotate variants, then reverse every complete cycle. Five repetitions do not exactly balance every position.',
                 statistics='mean_ns and sample stddev_ns describe all retained samples; repetition_mean_stddev_ns is sample SD across run means, not a confidence interval.',
                 metric='Whole concurrent batch latency, including construction, all service hops, validation, response destruction and ten-second deadline.',
                 goal='Every uninstrumented portable cell minimum valid latency <= the fastest reference on the same transport (legacy and C++ for TLS, legacy for QUIC); diagnostic timings do not qualify.',
                 trials=[])
    (out / 'scope.json').write_text(json.dumps(scope, indent=2) + '\n')

    def launch(label, argv, prefix, cpu):
        env = dict(os.environ, CAPNTPROTO_BENCH_CERTS=str(certificates), CAPNTPROTO_BENCH_TRANSPORT=args.transport, CAPNTPROTO_BENCH_PORTABLE='1' if label in ['portable', 'portable-baseline'] else '0')
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
                command = (['comparison-topology-server', label] if label in ['grpc', 'websocket'] else ['topology-server']) + ['127.0.0.1:0'] + ([address] if address else [])
                process = launch(label, command, directory / f'service-{hop}', cpu)
                workers.append(process)
                with selectors.DefaultSelector() as selector:
                    selector.register(process.stdout, selectors.EVENT_READ)
                    assert selector.select(30), 'server readiness timeout'
                    line = process.stdout.readline()
                (directory / f'service-{hop}.ready.json').write_text(line)
                ready = json.loads(line)
                assert ready.get('security', {}).get('transport') == ('quic-v1' if args.transport == 'quic' else 'tls-tcp'), 'server transport mismatch'
                assert ready['hops'] == case['hops'] - hop, 'incorrect chain length'
                if label in ['portable', 'portable-baseline']:
                    assert ready['implementation'] == 'portable', 'portable service was not selected'
                elif 'implementation' in ready:
                    assert ready['implementation'] == (label if label in ['grpc', 'websocket'] else ('capnp-cpp' if label == 'cpp' else 'legacy'))
                address = ready.get('address') or f"127.0.0.1:{ready['port']}"
                placement.append(dict(hop=hop, implementation=label, cpu=cpu, address=address))
            command = (['comparison-topology-measure', client_label] if client_label in ['grpc', 'websocket'] else ['topology-measure'])
            client = launch(client_label, command + [address, case['name'], str(warm), str(count)], directory / 'client', cpus[0])
            output, _ = client.communicate(timeout=300)
            (directory / 'trial.json').write_text(output)
            assert client.returncode == 0, f'client failed: {directory}'
            data = json.loads(output)
            assert data.get('security', {}).get('transport') == ('quic-v1' if args.transport == 'quic' else 'tls-tcp'), 'client transport mismatch'
            assert data['topology_version'] == 1 and data['case'] == case
            expected = client_label if client_label in ['grpc', 'websocket'] else ('portable' if client_label in ['portable', 'portable-baseline'] else ('capnp-cpp' if client_label == 'cpp' else 'legacy'))
            assert data['implementation'] == expected
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
            for label in [k for k in binaries if k in ['legacy', 'portable', 'cpp']]:
                for other in [k for k in binaries if k in ['legacy', 'portable', 'cpp']]:
                    if label == other:
                        continue
                    services = [other if hop % 2 == 0 else label for hop in range(case['hops'])]
                    prefix = f"mixed-{case['name']}-{label}-{other}"
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
                                **latency_statistics([t['latency_ns'] for t in trials]),
                                repetition_p50_ns=[percentile(t['latency_ns'], 50) for t in trials],
                                batches_per_second=rate, logical_requests_per_second=rate * case['width'],
                                service_calls_per_second=rate * case['width'] * case['hops'])
        ratio = cells['legacy']['p50_ns'] / cells['cpp']['p50_ns'] if 'cpp' in cells else None
        reference = min([k for k in ['legacy', 'cpp'] if k in cells], key=lambda label: cells[label]['p50_ns'])
        portable_ratio = cells['portable']['p50_ns'] / cells[reference]['p50_ns']
        rows.append(dict(case=case, variants=cells, legacy_over_cpp=ratio, faster_reference=reference,
                         portable_over_faster_reference=portable_ratio,
                         meets_target=portable_ratio <= 1.0 if qualifying else None))
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
    lines = ['# Deep/concurrent RPC comparison', '', f'Transport: {args.transport}. C++ QUIC is unsupported.', '',
             f'{args.repetitions} independent process repetitions per implementation/case. Times are microseconds per complete batch.',
             'Minimum measured latency is the benchmark estimate. Mean ± SD is diagnostic and reports the mean latency and sample standard deviation of the repetition means. It is not a confidence interval.',
             'Per-sample SD, extrema, each run mean, run-mean range, p95/p99 and throughput are retained in summary.json.',
             'All repetitions and samples are included. Five repetitions cannot exactly balance all variant positions.', '',
             '| Case | Implementation | Minimum µs | Mean ± run SD µs | Run mean min–max µs | p50 µs | p95 µs | p99 µs |',
             '| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |']
    for row in rows:
        for label, cell in row['variants'].items():
            lo, hi = cell['repetition_mean_range_ns']
            lines.append(f"| {row['case']['name']} | {label} | {cell['min_ns']/1000:.3f} | {cell['mean_ns']/1000:.3f} ± {cell['repetition_mean_stddev_ns']/1000:.3f} | {lo/1000:.3f}–{hi/1000:.3f} | {cell['p50_ns']/1000:.3f} | {cell['p95_ns']/1000:.3f} | {cell['p99_ns']/1000:.3f} |")
    lines += ['', 'Instrumented/short runs are diagnostic only.' if not qualifying else
              'The latency target remains portable pooled p50 ≤ the fastest reference on the same transport in every cell; C++ is unavailable for QUIC.']
    (out / 'README.md').write_text('\n'.join(lines) + '\n')
    print(json.dumps(rows, indent=2))


if __name__ == '__main__':
    main()
