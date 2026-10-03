#!/usr/bin/env python3
"""Capture reproducible finite RPC experiments using a prebuilt probe binary."""
import argparse
import datetime
import hashlib
import json
from pathlib import Path
import platform
import subprocess


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True, help='New evidence directory')
    parser.add_argument('--binary', type=Path, default=Path('target/debug/examples/rpc_pipeline_probe'))
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[1]
    binary = args.binary.resolve(strict=True)
    args.output.mkdir(parents=True, exist_ok=False)
    outputs = []
    for _ in range(3):
        run = subprocess.run([str(binary)], capture_output=True, text=True, check=True, timeout=30)
        output = json.loads(run.stdout)
        assert len(output['scenarios']) == 15
        assert all(row['validated'] for row in output['scenarios'])
        outputs.append(output)
    assert outputs[0] == outputs[1] == outputs[2], 'Virtual-clock results differed across repetitions'
    sources = [
        'examples/rpc_pipeline_probe.rs', 'scripts/rpc_pipeline_probe.py',
        'Cargo.toml', 'Cargo.lock', 'schemas/runtime-test.capnp', 'test-support/build.rs',
        'vendor/provenance/revision.json', 'vendor/provenance/capnp-rpc-revision.json',
        'vendor/capnp-rpc/src/rpc.rs', 'vendor/capnp-rpc/src/queued.rs',
        'vendor/capnp-rpc/src/local.rs', 'vendor/capnp-rpc/src/pipeline_builder.rs',
        'vendor/capnp-rpc/src/twoparty.rs', 'vendor/capnp-futures/src/write_queue.rs',
        'vendor/capnp-futures/src/serialize.rs', 'vendor/capnp-futures/src/buffered_read.rs',
        'vendor/capnp/src/capability.rs', 'vendor/capnpc/src/codegen.rs',
    ]
    environment = {
        'captured_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
        'platform': platform.platform(),
        'rustc': subprocess.check_output(['rustc', '--version'], text=True).strip(),
        'git_head': subprocess.check_output(['git', '-C', str(root), 'rev-parse', 'HEAD'], text=True).strip(),
        'binary_sha256': digest(binary),
        'sources_sha256': {name: digest(root / name) for name in sources},
        'identical_repetitions': 3,
        'notes': 'Finite protocol scenarios, not wall-clock benchmarks. Tokio paused time advances '
                 'only when execution has no ready work. Per-frame delays overlap, preserve order '
                 'and model propagation without bandwidth, loss, jitter, TLS or QUIC. '
                 'Zero-delay virtual durations do not measure CPU cost. Sources are selected central '
                 'inputs, not every transitive file; Git HEAD alone does not identify this dirty checkout.',
    }
    (args.output / 'environment.json').write_text(json.dumps(environment, indent=2) + '\n')
    (args.output / 'results.json').write_text(json.dumps(outputs[0], indent=2) + '\n')
    print('15 scenarios validated; three fresh processes produced identical virtual times and wire counters.')


if __name__ == '__main__':
    main()
