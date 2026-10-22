#!/usr/bin/env python3
"""Reassemble every TCP edge in topology-runner.py's full wire captures."""
import argparse
from collections import Counter, defaultdict
import gzip
import hashlib
import json
from pathlib import Path
import re
import struct

KINDS = ['unimplemented', 'abort', 'call', 'return', 'finish', 'resolve', 'release',
         'obsoleteSave', 'bootstrap', 'obsoleteDelete', 'provide', 'accept', 'join',
         'disembargo', 'thirdPartyAnswer']
READS = {'read', 'readv', 'recvfrom', 'recvmsg'}


class Stream:
    def __init__(self):
        self.pending = bytearray()
        self.digest = hashlib.sha256()
        self.size = 0
        self.frames = []

    def add(self, data):
        self.digest.update(data)
        self.size += len(data)
        self.pending.extend(data)
        while len(self.pending) >= 8:
            segments = struct.unpack_from('<I', self.pending)[0] + 1
            assert 1 <= segments <= 512, 'invalid segment table'
            header = (segments // 2 + 1) * 8
            if len(self.pending) < header:
                break
            words = sum(struct.unpack_from('<' + 'I' * segments, self.pending, 4))
            size = header + words * 8
            if len(self.pending) < size:
                break
            frame = self.pending[:size]
            # These RPC envelopes use a near struct root. Reject an unfamiliar
            # envelope rather than silently guessing its message discriminator.
            pointer, = struct.unpack_from('<Q', frame, header)
            assert pointer & 3 == 0 and (pointer >> 32) & 65535 >= 1
            offset = (pointer >> 2) & 0x3fffffff
            if offset & (1 << 29):
                offset -= 1 << 30
            target = header + 8 + offset * 8
            segment_end = header + struct.unpack_from('<I', frame, 4)[0] * 8
            assert header <= target and target + 2 <= segment_end
            kind, = struct.unpack_from('<H', frame, target)
            assert kind < len(KINDS)
            self.frames.append(dict(kind=KINDS[kind], bytes=size,
                                    sha256=hashlib.sha256(frame).hexdigest()))
            del self.pending[:size]

    def result(self):
        assert not self.pending, 'capture ends inside a frame'
        return dict(bytes=self.size, sha256=self.digest.hexdigest(), frames=self.frames)


def decode(path):
    pending, streams, identities = {}, defaultdict(Stream), {}
    counts = Counter()
    with gzip.open(path, 'rt') as lines:
        for line in lines:
            match = re.match(r'(\d+)\s+(\d+\.\d+)\s+(.*)', line)
            if not match:
                continue
            pid, _, body = match.groups()
            if '<unfinished ...>' in body:
                assert pid not in pending
                pending[pid] = (line.split('<unfinished ...>', 1)[0], body.split('(', 1)[0])
                continue
            resumed = re.match(r'<\.\.\. (\w+) resumed>(.*)', body)
            if resumed:
                prefix, operation = pending.pop(pid)
                assert operation == resumed[1]
                line = prefix + resumed[2]
            match = re.search(r' (read|write|readv|writev|sendto|recvfrom|sendmsg|recvmsg)\((\d+)<TCP:\[([^]]+)\]>.*\)\s+= (-?\d+)', line)
            if not match:
                continue
            operation, fd, identity, amount = match.groups()
            # The fixture keeps every connection alive for the whole trial.
            # Refuse descriptor reuse or inconsistent decoding. In restricted
            # namespaces strace may expose only an inode, even for live TCP.
            assert identities.setdefault(fd, identity) == identity, 'socket identity changed'
            amount = int(amount)
            direction = 'read' if operation in READS else 'write'
            counts[direction + '_attempts'] += 1
            if amount <= 0:
                continue
            counts[direction + '_positive'] += 1
            data = b''.join(bytes.fromhex(part.replace('\\x', ''))
                            for part in line.split('"')[1::2] if part.startswith('\\x'))
            assert len(data) >= amount, f'truncated payload: {path}'
            streams[fd, direction].add(data[:amount])
    return counts, streams, identities, [operation for _, operation in pending.values()]


def matches(sent, received):
    return (bool(received) and received == sent[:len(received)] and
            all(f['kind'] in {'finish', 'release', 'abort'} for f in sent[len(received):]))


def adjacent(first, second):
    level = lambda name: -1 if name == 'client' else int(name.removeprefix('service-'))
    return abs(level(first) - level(second)) == 1


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory', type=Path)
    args = parser.parse_args()
    scope = json.loads((args.directory / 'scope.json').read_text())
    assert not (scope.get('transport') or scope.get('security_version')), 'encrypted captures contain ciphertext; use syscall accounting, not plaintext frame decoding'
    assert scope['instrument'] == 'wire'
    rows = []
    for trial in scope['trials']:
        directory = args.directory / trial['path']
        connections, processes = {}, []
        for path in sorted(directory.glob('*.strace.gz')):
            counts, streams, identities, unfinished = decode(path)
            process = path.name.removesuffix('.strace.gz')
            processes.append(dict(process=path.name, counts=dict(counts),
                                  unfinished_at_exit=unfinished))
            for (fd, direction), stream in streams.items():
                connection = connections.setdefault((process, fd), dict(identity=identities[fd]))
                assert direction not in connection
                connection[direction] = stream.result()
        checks = []
        while connections:
            source, first = connections.popitem()
            assert {'read', 'write'} <= first.keys(), (directory, source)
            # Pair only adjacent service processes, requiring a unique full
            # bidirectional payload match. Never pair by startup frame alone.
            peers = [(key, other) for key, other in connections.items()
                     if adjacent(source[0], key[0]) and {'read', 'write'} <= other.keys()
                     and matches(first['write']['frames'], other['read']['frames'])
                     and matches(other['write']['frames'], first['read']['frames'])]
            assert len(peers) == 1, (directory, source, 'missing or ambiguous peer', len(peers))
            destination, second = peers[0]
            del connections[destination]
            for start, end, writer, reader in [(source, destination, first, second),
                                              (destination, source, second, first)]:
                sent, received = writer['write']['frames'], reader['read']['frames']
                checks.append(dict(source=start, destination=end, source_socket=writer['identity'],
                                   destination_socket=reader['identity'], received_frames=len(received),
                                   sent_frames=len(sent), received_bytes=reader['read']['bytes'],
                                   sent_bytes=writer['write']['bytes'], received_sha256=reader['read']['sha256'],
                                   sent_sha256=writer['write']['sha256'], kinds=dict(Counter(f['kind'] for f in sent)),
                                   all_received_frames_match=True, unread_teardown=sent[len(received):]))
        case = next(c for c in scope['cases'] if c['name'] == trial['case'])
        assert len(checks) == 2 * (case['connections'] + case['hops'] - 1)
        rows.append(dict(trial=trial['path'], processes=processes, edges=checks))
    output = args.directory / 'wire-analysis.json'
    with output.open('x') as sink:
        json.dump(dict(analyzer_sha256=hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                       trials=rows), sink, indent=2)
        sink.write('\n')
    print(f'Checked {len(rows)} trials, {sum(len(r["edges"]) for r in rows)} directed TCP edges')


if __name__ == '__main__':
    main()
