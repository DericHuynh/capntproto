"""Analyze separate socket diagnostics; never modify qualification results."""
import argparse
import hashlib
import json
import statistics
import struct
from pathlib import Path

EVENT = struct.Struct('<QQQQqiIII')


def intervals(raw):
    if len(raw) < 8:
        raise ValueError('missing journal header')
    count, = struct.unpack_from('<Q', raw)
    if count > 131072 or len(raw) != 8 + EVENT.size * count:
        raise ValueError('truncated or overflowing journal')
    pending, wall, cpu, threads = {}, [], [], set()
    for start, end, cs, ce, result, fd, op, tid, reserved in EVENT.iter_unpack(raw[8:]):
        if not (0 < start <= end and 0 < cs <= ce and tid > 0 and reserved == 0 and 1 <= op <= 12):
            raise ValueError('invalid event')
        if result <= 0:
            continue
        key = tid, fd
        if op in (1, 3, 5, 7, 9):
            pending[key] = end, ce
        elif op in (2, 4, 6, 8, 10) and key in pending:
            read_wall, read_cpu = pending.pop(key)
            if start < read_wall or cs < read_cpu:
                raise ValueError('nonmonotonic paired timestamps')
            wall.append(start - read_wall)
            cpu.append(cs - read_cpu)
            threads.add(tid)
    return dict(event_count=count, thread_ids=sorted(threads), wall_ns=wall, cpu_ns=cpu)


def analyze(root, manifest):
    scope = json.loads((root / 'scope.json').read_text())
    expected = manifest['binaries']
    sha = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
    for peer in ('client', 'server'):
        directory = root / peer / 'timing'
        hashes = {}
        for line in (directory / 'hashes.txt').read_text().splitlines():
            digest, name = line.split()
            if name in hashes:
                raise ValueError('duplicate recorded digest')
            hashes[name] = digest
        assert set(hashes) == {'probe.c', 'probe.so', '/root/bench/driver', '/root/bench/capnp-reference'}
        assert hashes['probe.c'] == sha(directory / 'probe.c') == scope['probe_source_sha256']
        assert hashes['probe.so'] == sha(directory / 'probe.so')
        for name in ('driver', 'capnp-reference'):
            assert hashes['/root/bench/' + name] == expected[name]
    rows = []
    for repetition in range(scope['repetitions']):
        for case in scope['cases']:
            for variant in scope['variants']:
                prefix = f'{repetition}-{case}-{variant}'
                trial = json.loads((root / 'client/timing' / (prefix + '-client.json')).read_text())
                assert trial['iterations'] == scope['iterations'] == len(trial['latency_ns'])
                for peer in ('client', 'server'):
                    row = intervals((root / peer / 'timing' / (prefix + f'-{peer}.bin')).read_bytes())
                    assert len(row['wall_ns'][210:-1]) > 500
                    rows.append(dict(case=case, variant=variant, peer=peer, repetition=repetition, **row))
    summary = []
    for case in scope['cases']:
        for variant in scope['variants']:
            for peer in ('client', 'server'):
                cells = [r for r in rows if (r['case'], r['variant'], r['peer']) == (case, variant, peer)]
                result = dict(case=case, variant=variant, peer=peer)
                for clock in ('wall', 'cpu'):
                    values = sorted(n for cell in cells for n in cell[clock + '_ns'][210:-1])
                    result[clock + '_p50_ns'] = statistics.median(values)
                    result[clock + '_p95_ns'] = values[(len(values) * 95 + 99) // 100 - 1]
                    result[clock + '_repetition_p50_ns'] = [statistics.median(c[clock + '_ns'][210:-1]) for c in cells]
                summary.append(result)
    (root / 'rows.json').write_text(json.dumps(rows, indent=2) + '\n')
    (root / 'summary.json').write_text(json.dumps(dict(scope=scope['scope'], identities_verified=True, rows=summary), indent=2) + '\n')
    return summary


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('root', type=Path)
    parser.add_argument('--manifest', type=Path, required=True)
    args = parser.parse_args()
    manifest = json.loads(args.manifest.read_text())
    print(json.dumps(analyze(args.root, manifest), indent=2))
