#!/usr/bin/env python3
"""End-to-end encrypted fixture checks, including authentication failures and mixed RPC engines."""
import argparse
import importlib.util
import json
import selectors
import subprocess
from pathlib import Path

HERE = Path(__file__).resolve().parent
spec = importlib.util.spec_from_file_location('secure_runner', HERE / 'secure-runner.py')
runner = importlib.util.module_from_spec(spec)
spec.loader.exec_module(runner)

def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--rust', required=True, type=Path)
    p.add_argument('--cpp', required=True, type=Path)
    p.add_argument('--upstream', type=Path)
    p.add_argument('--output', required=True, type=Path)
    args = p.parse_args()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    certs = runner.certificates.generate(output / 'certificates')
    wrong = runner.certificates.generate(output / 'untrusted')
    binaries = dict(legacy=args.rust.resolve(strict=True), portable=args.rust.resolve(strict=True), cpp=args.cpp.resolve(strict=True),
                    grpc=args.rust.resolve(strict=True), websocket=args.rust.resolve(strict=True))
    if args.upstream:
        binaries['upstream'] = args.upstream.resolve(strict=True)
    campaign = runner.Campaign(binaries, output, certs, runner.physical_cpus()[0])
    records = []
    def record(name, **data):
        records.append(dict(name=name, **data))
        (output / 'checks.json').write_text(json.dumps(records, indent=2) + '\n')
    for transport in ['tls', 'quic']:
        for case in runner.catalog():
            for label in binaries:
                if runner.supported(label, transport, case):
                    name = f'{transport}-{case["name"]}-{label}'
                    campaign.trial(case, label, transport, name, 1, 2)
                    record(name, passed=True)
        # Callback, pipelining and multi-hop concurrent forwarding across engines.
        labels = ['legacy', 'portable'] + (['cpp'] if transport == 'tls' else [])
        for case in runner.catalog():
            if case['name'] not in ['loopback-64', 'pipelined-read', 'callback', 'services-4-deep-16-clients-8']:
                continue
            for client in labels:
                for server in labels:
                    if client == server:
                        continue
                    name = f'mixed-{transport}-{case["name"]}-{client}-{server}'
                    campaign.trial(case, client, transport, name, 1, 2,
                                   services=[server if i % 2 == 0 else client for i in range(case.get('hops',1))])
                    record(name, passed=True)
        case = runner.catalog()[1]
        for label in binaries:
            if not runner.supported(label, transport, case):
                continue
            for reason, overrides in [('hostname', {'CAPNTPROTO_BENCH_SERVER_NAME':'wrong.invalid'}),
                                      ('trust-root', {'CAPNTPROTO_BENCH_CERTS':str(wrong)})]:
                name = f'reject-{transport}-{label}-{reason}'
                result = campaign.trial(case, label, transport, name, 0, 1, client_overrides=overrides, reject=True)
                record(name, **result)
    # Independently inspect actual TLS negotiation and reject TLS 1.2 on every TCP fixture.
    case = runner.catalog()[1]
    for label in binaries:
        for version in ['tls1_3', 'tls1_2']:
            name = f'negotiation-{label}-{version}'
            directory = output / name
            directory.mkdir()
            worker = campaign.launch(label, campaign.server_args(label, case), 'tls', directory, 'server', campaign.cpus[1])
            try:
                with selectors.DefaultSelector() as selector:
                    selector.register(worker.stdout, selectors.EVENT_READ)
                    assert selector.select(30), 'server readiness timeout'
                    ready = json.loads(worker.stdout.readline())
                result = subprocess.run(['openssl', 's_client', '-brief', '-connect', ready['address'],
                    '-servername', 'localhost', '-verify_hostname', 'localhost', '-verify_return_error',
                    '-CAfile', str(certs/'ca.pem'), '-' + version, '-alpn', 'h2'],
                    input='', capture_output=True, text=True, timeout=15)
                (directory/'openssl.txt').write_text(result.stdout + result.stderr)
                if version == 'tls1_3':
                    assert 'Protocol version: TLSv1.3' in result.stderr and 'Verification: OK' in result.stderr
                else:
                    assert result.returncode != 0 and 'Protocol version: TLSv1.2' not in result.stderr
                record(name, passed=True, exit=result.returncode)
            finally:
                runner.stop(worker)
    print(json.dumps(dict(checks=len(records), passed=True)))

if __name__ == '__main__':
    main()
