#!/usr/bin/env python3
"""Count TCP/UDP syscalls in secure-runner traces. No ciphertext decoding or timing qualification."""
import argparse
import collections
import json
from pathlib import Path
import re

START = re.compile(r'^(\d+)\s+([0-9.]+)\s+(.*)$')
CALL = re.compile(r'^(read|write|readv|writev|sendto|recvfrom|sendmsg|recvmsg|recvmmsg|sendmmsg)\(.*\)\s+=\s+(-?\d+|\?)')
READS = {'read', 'readv', 'recvfrom', 'recvmsg', 'recvmmsg'}

def count(path):
    counts, pending = collections.Counter(), {}
    for line in path.read_text().splitlines():
        match = START.match(line)
        if not match:
            continue
        pid, _, body = match.groups()
        if '<unfinished ...>' in body:
            pending[pid] = body.replace('<unfinished ...>', '')
            continue
        if body.startswith('<... '):
            start = pending.pop(pid, None)
            if start is None:
                continue
            body = start + body.split('resumed>', 1)[1].lstrip()
        match = CALL.match(body)
        if not match or not re.search(r'<(?:TCP|UDP):', body):
            continue
        operation = match.group(1)
        result = None if match.group(2) == '?' else int(match.group(2))
        direction = 'read' if operation in READS else 'write'
        counts[direction + '_attempts'] += 1
        counts[direction + ('_unreturned' if result is None else ('_positive' if result > 0 else '_nonpositive'))] += 1
        counts['syscall_' + operation + '_attempts'] += 1
        if result is not None and result > 0:
            if operation in ['recvmmsg', 'sendmmsg']:
                counts[direction + '_batched_datagrams'] += result
            else:
                counts[direction + '_returned_bytes'] += result
    for body in pending.values():
        if re.search(r'<(?:TCP|UDP):', body):
            operation = body.split('(', 1)[0]
            if operation in READS or operation in ['write', 'writev', 'sendto', 'sendmsg', 'sendmmsg']:
                direction = 'read' if operation in READS else 'write'
                counts[direction + '_attempts'] += 1
                counts[direction + '_unreturned'] += 1
                counts['syscall_' + operation + '_attempts'] += 1
    return dict(counts=counts, unfinished_socket_calls=sum(bool(re.search(r'<(?:TCP|UDP):', s)) for s in pending.values()))

def summarize(directory):
    journals = {str(p.relative_to(directory)):count(p) for p in sorted(directory.glob('*/*.strace'))}
    assert journals, 'no syscall captures'
    result = dict(scope='Entire peer lifecycle, including handshake, ciphertext, shutdown, QUIC ACK/retry/control packets. Attempts include errors, would-block and unreturned calls at termination; unreturned calls are counted separately. Batched UDP returns datagrams, not bytes. No Capn Proto message count inference.', journals=journals)
    (directory/'ciphertext-counts.json').write_text(json.dumps(result, indent=2) + '\n')
    return result

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory', type=Path)
    summarize(parser.parse_args().directory)
