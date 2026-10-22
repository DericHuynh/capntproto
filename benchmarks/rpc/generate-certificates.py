#!/usr/bin/env python3
"""Generate a fresh, private benchmark CA and localhost server identity. Never deploy these keys."""
import argparse
import json
import hashlib
import os
from pathlib import Path
import subprocess

def generate(directory):
    directory.mkdir(parents=True, mode=0o700, exist_ok=False)
    def run(*args):
        subprocess.run(['openssl', *args], cwd=directory, check=True, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
    old = os.umask(0o077)
    try:
        run('req', '-x509', '-newkey', 'ec', '-pkeyopt', 'ec_paramgen_curve:P-256', '-nodes',
            '-keyout', 'ca-key.pem', '-out', 'ca.pem', '-days', '7', '-subj', '/CN=Capntproto benchmark CA',
            '-addext', 'basicConstraints=critical,CA:TRUE', '-addext', 'keyUsage=critical,keyCertSign,cRLSign')
        run('req', '-newkey', 'ec', '-pkeyopt', 'ec_paramgen_curve:P-256', '-nodes',
            '-keyout', 'server-key.pem', '-out', 'server.csr', '-subj', '/CN=localhost')
        (directory / 'extensions.cnf').write_text('basicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature\nextendedKeyUsage=serverAuth\nsubjectAltName=DNS:localhost,IP:127.0.0.1,IP:::1\n')
        run('x509', '-req', '-in', 'server.csr', '-CA', 'ca.pem', '-CAkey', 'ca-key.pem', '-CAcreateserial',
            '-out', 'server.pem', '-days', '7', '-extfile', 'extensions.cnf')
        for name in ['ca', 'server']:
            run('x509', '-in', name + '.pem', '-outform', 'DER', '-out', name + '.der')
        run('pkcs8', '-topk8', '-nocrypt', '-in', 'server-key.pem', '-outform', 'DER', '-out', 'server-key.der')
        # No signing authority is needed after fixture creation.
        (directory / 'ca-key.pem').unlink()
        (directory / 'manifest.json').write_text(json.dumps({
            'benchmark_only': True, 'authentication': 'server-certificate', 'server_name': 'localhost',
            'certificate_sha256': {n: hashlib.sha256((directory / n).read_bytes()).hexdigest() for n in ['ca.der', 'server.der']}
        }, indent=2) + '\n')
    finally:
        os.umask(old)
    return directory

if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory', type=Path)
    generate(parser.parse_args().directory.resolve())
