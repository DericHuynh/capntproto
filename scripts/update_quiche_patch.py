#!/usr/bin/env python3
"""Refresh the complete quiche fork patch without staging or changing its checkout."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parent.parent
CHECKOUT = ROOT/'vendor/quiche'

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        '--git-dir', type=Path, default=CHECKOUT/'.git',
        help='Git metadata for a checkout at the pinned upstream revision; '
             'keep this outside the vendored source tree',
    )
    args = parser.parse_args()
    if not args.git_dir.is_dir():
        parser.error('vendored quiche has no Git metadata; pass --git-dir '
                     '/path/to/pinned-upstream-checkout/.git (see docs/FORK_POLICY.md)')
    git = ['git', '--git-dir', str(args.git_dir.resolve()),
           '--work-tree', str(CHECKOUT)]
    provenance = ROOT/'vendor/provenance/quiche-revision.json'
    value = json.loads(provenance.read_text())
    head = subprocess.check_output(git+['rev-parse','HEAD'],cwd=CHECKOUT,text=True).strip()
    if head != value['revision']:
        raise RuntimeError('quiche HEAD differs from recorded upstream revision')
    patch = subprocess.check_output(git+['diff','--binary','HEAD'],cwd=CHECKOUT)
    files = subprocess.check_output(git+['ls-files','--others','--exclude-standard','-z'],cwd=CHECKOUT).split(b'\0')
    for raw in files:
        if not raw: continue
        name = raw.decode()
        # Restrict archived additions to the fork's source/build inputs.
        corpus = name == 'quiche/src/simulation/packet/property-corpus.json'
        source = name.startswith('quiche/src/') and (name.endswith('.rs') or corpus)
        if not source and name != 'Cargo.lock':
            raise RuntimeError(f'unreviewed untracked fork file: {name}')
        result = subprocess.run(['git','diff','--no-index','--binary','--','/dev/null',name],cwd=CHECKOUT,stdout=subprocess.PIPE)
        if result.returncode != 1:
            raise RuntimeError(f'could not archive {name}')
        patch += result.stdout
    target = ROOT/'vendor/provenance/quiche-noise.patch'
    target.write_bytes(patch)
    subprocess.run(git+['apply','--reverse','--check',str(target)],cwd=CHECKOUT,check=True)
    value['patch_sha256'] = hashlib.sha256(patch).hexdigest()
    provenance.write_text(json.dumps(value,indent=2)+'\n')
    print(f'Archived {len(patch)} bytes; reverse application verified')

if __name__=='__main__': main()
