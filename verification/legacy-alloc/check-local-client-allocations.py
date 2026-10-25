#!/usr/bin/env python3
"""Compare real alloc-only client construction using isolated source snapshots."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil
import statistics
import subprocess
import tempfile

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--baseline', required=True)
parser.add_argument('--output', type=Path, required=True)
options = parser.parse_args()
if not re.fullmatch(r'[0-9a-f]{40}', options.baseline):
    raise SystemExit('baseline must be a full commit SHA')
repo = Path(__file__).resolve().parents[2]
subject = 'crates/capntproto-rpc/src/local.rs'
original = subprocess.check_output(['git','show',f'{options.baseline}:{subject}'],cwd=repo)
candidate = (repo/subject).read_bytes()
results = {}
with tempfile.TemporaryDirectory(prefix='legacy-client-') as directory:
    snapshot = Path(directory)
    crates = snapshot/'crates'
    crates.mkdir()
    # Only RPC is copied. Dependencies share exactly the current source and
    # feature profile; neither comparison edits the real working checkout.
    for crate in (repo/'crates').iterdir():
        if crate.is_dir() and crate.name!='capntproto-rpc':
            (crates/crate.name).symlink_to(crate,target_is_directory=True)
    rpc = crates/'capntproto-rpc'
    shutil.copytree(repo/'crates/capntproto-rpc',rpc,ignore=shutil.ignore_patterns('target'))
    fixture = snapshot/'verification/legacy-alloc'
    fixture.parent.mkdir()
    shutil.copytree(repo/'verification/legacy-alloc',fixture,ignore=shutil.ignore_patterns('target','__pycache__'))
    for label, source in [('baseline',original),('candidate',candidate)]:
        (rpc/'src/local.rs').write_bytes(source)
        run = subprocess.run(['cargo','test','--locked','--quiet','--manifest-path',str(fixture/'Cargo.toml'),'--lib','report_local_client_allocations','--','--nocapture','--test-threads=1'],cwd=repo,text=True,capture_output=True)
        if run.returncode:
            raise SystemExit(run.stdout+run.stderr)
        rows = re.findall(r'CLIENT_ALLOCATION_METRICS (\{[^\n]+\})',run.stdout)
        if len(rows)!=1:
            raise SystemExit(f'{label}: expected one allocation report')
        row=json.loads(rows[0])
        for metric in ['blocks','bytes']:
            assert len(row[metric+'_samples'])==5, row
            row[metric+'_mean']=statistics.mean(row[metric+'_samples'])
            row[metric+'_sd']=statistics.stdev(row[metric+'_samples'])
        results[label]=row
for before,after in zip(results['baseline']['blocks_samples'],results['candidate']['blocks_samples']):
    assert before-after>=32,results
for before,after in zip(results['baseline']['bytes_samples'],results['candidate']['bytes_samples']):
    assert before>after,results
report=dict(baseline=options.baseline,subject=subject,
            original_sha256=hashlib.sha256(original).hexdigest(),
            candidate_sha256=hashlib.sha256(candidate).hexdigest(),
            repetitions=5,simultaneously_retained_clients=32,results=results,
            scope='Actual alloc-only ClientRegistry and generated Node clients. Outer registry capacity warmed before tracing. Includes service owners, dispatch state, per-interface maps and boxed hooks. Other RPC owners remain; no timing claim.')
options.output.parent.mkdir(parents=True,exist_ok=True)
options.output.write_text(json.dumps(report,indent=2)+'\n')
print(json.dumps(results,sort_keys=True))
