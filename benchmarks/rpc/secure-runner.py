#!/usr/bin/env python3
"""Matched TLS/QUIC RPC workloads. Creates certificates, retains every sample, rejects plaintext results."""
import argparse
import fcntl
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import selectors
import signal
import subprocess
import time
from latency_stats import summarize

HERE = Path(__file__).resolve().parent

def load(name, filename):
    spec = importlib.util.spec_from_file_location(name, HERE / filename)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module

topology = load('topology_runner', 'topology-runner.py')
certificates = load('certificates', 'generate-certificates.py')

def security(transport, protocol='tcp', name='localhost'):
    return dict(version=1, transport='quic-v1' if transport == 'quic' else 'tls-tcp', tls='1.3',
                authentication='server-certificate', server_name=name, early_data=False,
                setup_timed=False, alpn='capntproto-rpc/1' if transport == 'quic' else ('h2' if protocol == 'grpc' else None))

def engine(label):
    return {'portable-baseline': 'portable', 'legacy-baseline': 'legacy'}.get(label, label)


def validate(data, case, transport, label, warmup, iterations):
    label = engine(label)
    expected_protocol = label if label in ['grpc', 'websocket'] else 'tcp'
    assert data.get('security') == security(transport, expected_protocol), 'missing or mismatched encrypted transport identity'
    if case['family'] == 'loopback':
        assert data['measurement_version'] == 2 and data['payload_bytes'] == case['payload_bytes']
        assert data['protocol'] == {'cpp':'capnp-cpp', 'upstream':'capnp-rust'}.get(label, expected_protocol)
    else:
        assert data['case'] == {k:v for k,v in case.items() if k != 'family'}
        version = 'workflow_version' if case['family'] == 'workflow' else 'topology_version'
        assert data[version] == (3 if case['family'] == 'workflow' else 1)
        assert data['implementation'] == {'cpp':'capnp-cpp', 'upstream':'capnp-rust'}.get(label, label)
    samples = data['latency_ns']
    assert data['warmup'] == warmup and data['iterations'] == iterations == len(samples)
    assert all(type(n) is int and n > 0 for n in samples)
    assert data['elapsed_ns'] >= sum(samples)


def stop(process):
    if process.poll() is None:
        os.killpg(process.pid, signal.SIGTERM)
    try:
        process.wait(timeout=10)
    except subprocess.TimeoutExpired:
        os.killpg(process.pid, signal.SIGKILL)
        process.wait(timeout=10)


def campaign_lock(path=Path('/tmp/capntproto-secure-benchmark.lock')):
    lock = path.open('a')
    try:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError:
        lock.close()
        raise RuntimeError('another encrypted benchmark campaign is active') from None
    return lock


class Campaign:
    def __init__(self, binaries, output, certs, cpus, instrument=False):
        self._lock = campaign_lock()
        self.binaries, self.output, self.certs, self.cpus = binaries, output, certs, cpus
        self.instrument = instrument

    def launch(self, label, argv, transport, directory, role, cpu, overrides=None):
        env = dict(os.environ, CAPNTPROTO_BENCH_CERTS=str(self.certs), CAPNTPROTO_BENCH_TRANSPORT=transport,
                   CAPNTPROTO_BENCH_PORTABLE='1' if engine(label) == 'portable' else '0', CAPNTPROTO_BENCH_SERVER_NAME='localhost')
        env.update(overrides or {})
        command = ['taskset', '-c', str(cpu)]
        if self.instrument:
            # Ciphertext syscall trace, not a Cap'n Proto frame decoder. QUIC includes UDP ACKs/retries.
            command += ['strace', '-f', '-qq', '-ttt', '-T', '-yy', '-s', '64', '-e',
                        'trace=read,write,readv,writev,sendto,recvfrom,sendmsg,recvmsg,recvmmsg,sendmmsg,connect,accept,accept4',
                        '-o', str(directory / (role + '.strace'))]
        with (directory / (role + '.stderr')).open('x') as error:
            return subprocess.Popen([*command, str(self.binaries[label]), *argv], env=env,
                                    stdout=subprocess.PIPE, stderr=error, text=True, start_new_session=True)

    @staticmethod
    def server_args(label, case, downstream=None):
        family = case['family']
        if family == 'topology':
            prefix = ['comparison-topology-server', label] if label in ['grpc', 'websocket'] else ['topology-server']
            return prefix + ['127.0.0.1:0'] + ([downstream] if downstream else [])
        if family == 'workflow':
            return (['comparison-server', label] if label in ['grpc', 'websocket'] else ['workflow-server']) + ['127.0.0.1:0']
        return ['server'] + ([] if label in ['cpp', 'upstream'] else [label if label in ['grpc', 'websocket'] else 'tcp'])

    @staticmethod
    def client_args(label, case, address, warmup, iterations):
        family = case['family']
        if family == 'topology':
            prefix = ['comparison-topology-measure', label] if label in ['grpc', 'websocket'] else ['topology-measure']
            return prefix + [address, case['name'], str(warmup), str(iterations)]
        if family == 'workflow':
            return (['comparison-measure', label] if label in ['grpc', 'websocket'] else ['workflow-measure']) + [address, case['name'], str(warmup), str(iterations)]
        prefix = ['measure'] + ([] if label in ['cpp', 'upstream'] else [label if label in ['grpc', 'websocket'] else 'tcp'])
        return prefix + [address] + ([] if label == 'cpp' else ['null']) + [str(case['payload_bytes']), str(warmup), str(iterations)]

    def trial(self, case, label, transport, prefix, warmup, iterations, services=None, client_overrides=None, reject=False):
        directory = self.output / prefix
        directory.mkdir()
        workers, address, placement, client = [], None, [], None
        before = topology.host_state()
        try:
            hops = case.get('hops', 1)
            services = services or [label] * hops
            for hop in reversed(range(hops)):
                server_label = services[hop]
                cpu = self.cpus[1 + hop % (len(self.cpus) - 1)]
                worker = self.launch(server_label, self.server_args(server_label, case, address), transport, directory, f'service-{hop}', cpu)
                workers.append(worker)
                with selectors.DefaultSelector() as selector:
                    selector.register(worker.stdout, selectors.EVENT_READ)
                    assert selector.select(30), 'server readiness timeout'
                    line = worker.stdout.readline()
                (directory / f'service-{hop}.ready.json').write_text(line)
                ready = json.loads(line)
                expected = security(transport, server_label if server_label in ['grpc', 'websocket'] else 'tcp')
                assert ready.get('security') == expected, 'server is not using the requested encrypted transport'
                if case['family'] == 'topology':
                    assert ready['hops'] == hops - hop
                if engine(server_label) in ['legacy', 'portable']:
                    assert ready['implementation'] == engine(server_label)
                address = ready.get('address') or f"127.0.0.1:{ready['port']}"
                placement.append(dict(hop=hop, implementation=server_label, cpu=cpu, address=address))
            client = self.launch(label, self.client_args(label, case, address, warmup, iterations), transport, directory,
                                 'client', self.cpus[0], client_overrides)
            output, _ = client.communicate(timeout=300)
            (directory / 'client.stdout').write_text(output)
            if reject:
                assert client.returncode != 0 and not output.strip(), 'unauthenticated client produced benchmark results'
                return dict(rejected=True, client_exit=client.returncode, services=placement)
            assert client.returncode == 0, f'client failed: {directory}'
            data = json.loads(output)
            validate(data, case, transport, label, warmup, iterations)
            # Loopback servers legitimately finish on client disconnect; persistent servers must remain live.
            if case['family'] != 'loopback':
                assert all(worker.poll() is None for worker in workers), 'service exited during trial'
            (directory / 'trial.json').write_text(json.dumps(data) + '\n')
            return dict(data=data, services=placement)
        finally:
            if client is not None:
                stop(client)
            for worker in reversed(workers):
                stop(worker)
            (directory / 'host.json').write_text(json.dumps(dict(before=before, after=topology.host_state()), indent=2) + '\n')


def physical_cpus():
    rows = subprocess.check_output(['lscpu', '-b', '-p=CPU,CORE,SOCKET'], text=True)
    cores, cpus = set(), []
    for row in rows.splitlines():
        if not row or row.startswith('#'):
            continue
        cpu, core, socket = map(int, row.split(','))
        if cpu in os.sched_getaffinity(0) and (core, socket) not in cores:
            cpus.append(cpu)
            cores.add((core, socket))
    assert len(cpus) >= 2
    return cpus, rows


def idle_host():
    for path in Path('/proc').glob('[0-9]*/comm'):
        try:
            name = path.read_text().strip()
            argv = (path.parent / 'cmdline').read_bytes().split(b'\0')
            modes = [b'server', b'measure', b'workflow-server', b'workflow-measure',
                     b'topology-server', b'topology-measure', b'comparison-server', b'comparison-measure',
                     b'comparison-topology-server', b'comparison-topology-measure']
            assert len(argv) < 2 or argv[1] not in modes, f'competing benchmark peer: {path.parent}'
            if name in ['rustc', 'cargo', 'clippy-driver', 'rust-lld', 'clang', 'clang++', 'cc1plus', 'strace'] or 'callgrind' in name:
                assert (path.parent / 'stat').read_text().split(') ', 1)[1].split()[0] == 'Z', f'competing build/profiler: {path}'
        except (FileNotFoundError, ProcessLookupError):
            pass


def catalog():
    return ([dict(family='loopback', name=f'loopback-{n}', payload_bytes=n) for n in [0,64,1024,65536]]
            + [dict(family='workflow', **c) for c in json.loads((HERE / 'workflows.json').read_text())]
            + [dict(family='topology', **c) for c in json.loads((HERE / 'topologies.json').read_text())])


def supported(label, transport, case):
    label = engine(label)
    if transport == 'quic' and label not in ['legacy', 'portable']:
        return False
    if case['family'] == 'topology' and label not in ['legacy', 'portable', 'cpp', 'grpc', 'websocket']:
        return False
    return not (case['name'] == 'pipelined-read' and label in ['grpc', 'websocket'])


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--rust', type=Path, required=True)
    parser.add_argument('--portable', type=Path, help='Candidate portable executable; defaults to --rust.')
    parser.add_argument('--legacy-baseline', type=Path, help='Frozen legacy control, separate from the candidate.')
    parser.add_argument('--portable-baseline', type=Path, help='Frozen portable control, separate from the candidate.')
    parser.add_argument('--cpp', type=Path, required=True)
    parser.add_argument('--upstream', type=Path)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--transport', choices=['tls','quic','both'], default='both')
    parser.add_argument('--cases', default='')
    parser.add_argument('--repetitions', type=int, default=5)
    parser.add_argument('--warmup', type=int, help='Override default 10000 loopback / 100 workflow and topology warmups.')
    parser.add_argument('--iterations', type=int, default=1000)
    parser.add_argument('--mixed', action='store_true')
    parser.add_argument('--comparators', action='store_true', help='Include HTTPS/gRPC, WSS, and optional upstream Rust TLS.')
    parser.add_argument('--trace', action='store_true', help='Diagnostic ciphertext syscall captures; timings never qualify.')
    args = parser.parse_args()
    assert 1 <= args.repetitions <= 100 and (args.warmup is None or 0 <= args.warmup <= 100000) and 1 <= args.iterations <= 100000
    idle_host()
    binaries = dict(legacy=args.rust.resolve(strict=True), portable=args.rust.resolve(strict=True), cpp=args.cpp.resolve(strict=True))
    if args.portable:
        binaries['portable'] = args.portable.resolve(strict=True)
    if args.portable_baseline:
        binaries['portable-baseline'] = args.portable_baseline.resolve(strict=True)
    if args.legacy_baseline:
        binaries['legacy-baseline'] = args.legacy_baseline.resolve(strict=True)
    if args.comparators:
        binaries.update(grpc=binaries['legacy'], websocket=binaries['legacy'])
        if args.upstream:
            binaries['upstream'] = args.upstream.resolve(strict=True)
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    certs = certificates.generate(output / 'certificates')
    cpus, cpu_topology = physical_cpus()
    campaign = Campaign(binaries, output, certs, cpus, args.trace)
    cases = catalog()
    if args.cases:
        selected = set(args.cases.split(','))
        assert selected <= {c['name'] for c in cases}, 'unknown case'
        cases = [c for c in cases if c['name'] in selected]
    transports = ['tls', 'quic'] if args.transport == 'both' else [args.transport]
    scope = dict(security_version=1, transports=transports, cases=cases, repetitions=args.repetitions,
                 comparison_version=1 if args.comparators else 0,
                 comparison_encoding='protobuf' if args.comparators else None,
                 warmup=args.warmup if args.warmup is not None else {'loopback':10000,'workflow':100,'topology':100}, iterations=args.iterations, cpus=cpus, cpu_topology=cpu_topology,
                 binaries={k:dict(path=str(v), sha256=topology.digest(v)) for k,v in binaries.items()},
                 certificate_manifest=json.loads((certs / 'manifest.json').read_text()),
                 sources={p.name:topology.digest(p) for p in [Path(__file__), HERE/'generate-certificates.py', HERE/'workflows.json', HERE/'topologies.json', HERE/'latency_stats.py']},
                 metric='Established-connection whole-operation latency; setup/handshake excluded, validation and cleanup included.',
                 order='Rotate variants then reverse every complete cycle; five repetitions do not exactly balance positions.',
                 unsupported=[dict(transport=t, implementation=k, case=c['name']) for t in transports for k in binaries for c in cases if not supported(k,t,c)],
                 diagnostic=args.trace, trials=[])
    def save():
        (output / 'scope.json').write_text(json.dumps(scope, indent=2) + '\n')
    save()
    if args.mixed:
        for transport in transports:
            labels = ['legacy', 'portable'] + (['cpp'] if transport == 'tls' else [])
            for case in cases:
                for client in labels:
                    for server in labels:
                        if client == server:
                            continue
                        prefix = f'mixed-{transport}-{case["name"]}-{client}-{server}'
                        services = [server if i % 2 == 0 else client for i in range(case.get('hops',1))]
                        result = campaign.trial(case, client, transport, prefix, 2, 5, services)
                        scope['trials'].append(dict(path=prefix, transport=transport, case=case['name'], client=client, services=result['services'], mixed=True))
            save()
    rows = []
    for transport in transports:
        for case in cases:
            warmup = args.warmup if args.warmup is not None else (10000 if case['family'] == 'loopback' else 100)
            labels = [k for k in binaries if supported(k, transport, case)]
            cells = {k:[] for k in labels}
            for rep in range(args.repetitions):
                order = labels[rep % len(labels):] + labels[:rep % len(labels)]
                if rep // len(labels) % 2:
                    order.reverse()
                for label in order:
                    prefix = f'{transport}-{case["name"]}-{rep}-{label}'
                    result = campaign.trial(case, label, transport, prefix, warmup, args.iterations)
                    cells[label].append(result['data']['latency_ns'])
                    scope['trials'].append(dict(path=prefix, transport=transport, case=case['name'], client=label, services=result['services'], mixed=False, repetition=rep))
                    save()
            row = dict(transport=transport, case=case, variants={k:dict(summarize(v), samples=sum(map(len,v)), **{f'p{p}_ns':topology.percentile([n for trial in v for n in trial],p) for p in [50,95,99]}) for k,v in cells.items()})
            rows.append(row)
            (output / 'summary.json').write_text(json.dumps(dict(rows=rows, diagnostic=args.trace), indent=2) + '\n')
            print(transport, case['name'], flush=True)
    lines = ['# Encrypted RPC comparison', '', 'Minimum measured operation latency is the benchmark result. Handshakes are excluded. Raw repetitions, means and spread are retained.',
             'gRPC uses Tonic/Protobuf over HTTP/2. WebSocket topologies use binary Protobuf with bounded, correlated concurrent requests. Both traverse the same nested request at every independent service hop, return the complete nested data, and validate checksums and visit counts. They use TLS 1.3, persistent connections, and the same depths, concurrency, client counts and repetition budgets.',
             'gRPC and WebSocket do not support capability promise pipelining or this suite\'s QUIC path; unsupported cells are listed in scope.json.',
             'C++ QUIC is unsupported at the pinned revision; no plaintext or TCP substitute is included.', '',
             '| Transport | Case | Implementation | Minimum µs | Mean ± run SD µs | Run mean min–max µs |',
             '| --- | --- | --- | ---: | ---: | ---: |']
    for row in rows:
        for label, cell in row['variants'].items():
            lo, hi = cell['repetition_mean_range_ns']
            lines.append(f'| {row["transport"]} | {row["case"]["name"]} | {label} | {cell["min_ns"]/1000:.3f} | {cell["mean_ns"]/1000:.3f} ± {cell["repetition_mean_stddev_ns"]/1000:.3f} | {lo/1000:.3f}–{hi/1000:.3f} |')
    if args.trace:
        lines += ['', 'Instrumented diagnostic timings do not qualify. Syscalls include setup, ciphertext, TLS records and QUIC control packets.']
    (output / 'README.md').write_text('\n'.join(lines) + '\n')
    if args.trace:
        load('ciphertext_counts', 'ciphertext-counts.py').summarize(output)

if __name__ == '__main__':
    main()
