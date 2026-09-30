#!/usr/bin/env python3
"""Install the pinned Apalache distribution after verifying its SHA-256."""
import hashlib
import json
from pathlib import Path
import tarfile
import tempfile
import urllib.request

ROOT = Path(__file__).resolve().parent
PIN = json.loads((ROOT / 'apalache.json').read_text())

def main():
    with tempfile.TemporaryDirectory(prefix='capnp-apalache-') as directory:
        archive = Path(directory) / 'apalache.tgz'
        urllib.request.urlretrieve(PIN['url'], archive)
        actual = hashlib.sha256(archive.read_bytes()).hexdigest()
        if actual != PIN['archive_sha256']:
            raise SystemExit('Apalache archive checksum mismatch')
        with tarfile.open(archive) as package:
            package.extractall(ROOT, filter='data')
    jar = ROOT / PIN['jar']
    if hashlib.sha256(jar.read_bytes()).hexdigest() != PIN['jar_sha256']:
        raise SystemExit('Apalache jar checksum mismatch')
    print(f'Installed Apalache {PIN["version"]}: {jar}')

if __name__ == '__main__':
    main()
