#!/usr/bin/env python3
"""Compare actual registry source revisions; this is not a latency benchmark."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import statistics
import subprocess
import tempfile

args = argparse.ArgumentParser(description=__doc__)
args.add_argument('--baseline', required=True)
args.add_argument('--output', type=Path, required=True)
options = args.parse_args()
if not re.fullmatch(r'[0-9a-f]{40}', options.baseline):
    raise SystemExit('baseline must be a full commit SHA')
repo = Path(__file__).resolve().parents[2]
source = 'crates/capntproto-rpc/src/system/registry.rs'
original = subprocess.check_output(['git', 'show', f'{options.baseline}:{source}'], cwd=repo).decode()
candidate = (repo / source).read_text()
exercise = '''
#[cfg(test)]
mod tests {
    use super::*;
    struct State;
    impl Connection for State { fn disconnect(&self, _: Error) {} }
    #[test]
    fn registration_allocation_trace() {
        let registry = Connections::<State>::new();
        registry.0.entries.borrow_mut().reserve(32);
        let counts: [_; 5] = core::array::from_fn(|_| allocation_counter::measure(|| {
            for _ in 0..4 {
                let registrations: [Registration<State>; 32] = core::array::from_fn(|id| {
                    match registry.acquire(id).unwrap() {
                        Acquired::New(registration) => registration,
                        Acquired::Existing(_) => panic!("expected a new registration"),
                    }
                });
                for registration in registrations { registration.complete(Ok(())); }
            }
        }));
        println!(r#"REGISTRATION_METRICS {{"revision":"REVISION","blocks_samples":{:?},"bytes_samples":{:?},"pointer_bytes":{}}}"#,
                 counts.each_ref().map(|count| count.count_total),
                 counts.each_ref().map(|count| count.bytes_total), core::mem::size_of::<usize>());
    }
}
'''
with tempfile.TemporaryDirectory(prefix='legacy-registration-') as folder:
    root = Path(folder)
    (root / 'src').mkdir()
    manifest = '''[package]
name="legacy-registration-allocation-check"
version="0.0.0"
edition="2021"
[workspace]
[features]
default=["std"]
std=[]
[dependencies]
capnp={package="capntproto-core",path=CORE}
capntproto-rpc-engine={path=ENGINE,features=["alloc"]}
futures={version="=0.3.34",default-features=false,features=["alloc","async-await","std"]}
allocation-counter="=0.8.1"
[profile.dev]
debug=0
incremental=false
'''.replace('CORE', json.dumps(str(repo/'crates/capntproto-core'))).replace('ENGINE', json.dumps(str(repo/'crates/capntproto-rpc-engine')))
    (root/'Cargo.toml').write_text(manifest)
    (root/'Cargo.lock').write_bytes((repo/'Cargo.lock').read_bytes())
    (root/'src/lib.rs').write_text('''#![allow(dead_code)]
extern crate alloc;
mod collections { pub(crate) use std::collections::HashMap; }
#[cfg(test)] mod baseline;
#[cfg(test)] mod candidate;
''')
    for label, code in [('baseline',original),('candidate',candidate)]:
        marker = '#[cfg(test)]\nmod tests;'
        if code.count(marker)!=1:
            raise SystemExit(f'{label}: unexpected registry test module boundary')
        (root/f'src/{label}.rs').write_text(code.replace(marker,exercise.replace('REVISION',label)))
    run = subprocess.run(['cargo','test','--quiet','--manifest-path',str(root/'Cargo.toml'),'--','--nocapture','--test-threads=1'],cwd=repo,text=True,capture_output=True)
    if run.returncode:
        raise SystemExit(run.stdout+run.stderr)
    rows = [json.loads(row) for row in re.findall(r'REGISTRATION_METRICS (\{[^\n]+\})',run.stdout)]
    if len(rows)!=2:
        raise SystemExit('expected one result for each revision')
    results={row['revision']:row for row in rows}
    for label, row in results.items():
        for metric in ['blocks', 'bytes']:
            samples = row[metric+'_samples']
            assert len(samples)==5, results
            row[metric+'_mean']=statistics.mean(samples)
            row[metric+'_sd']=statistics.stdev(samples)
    assert all(count==128 for count in results['baseline']['blocks_samples']), results
    assert all(count==128*2*results['baseline']['pointer_bytes'] for count in results['baseline']['bytes_samples']), results
    assert all(count==0 for metric in ['blocks','bytes'] for count in results['candidate'][metric+'_samples']), results
    report=dict(baseline=options.baseline,source=source,
                original_sha256=hashlib.sha256(original.encode()).hexdigest(),
                candidate_sha256=hashlib.sha256(candidate.encode()).hexdigest(),
                operations_per_repetition=128,concurrent=32,waves_per_repetition=4,repetitions=5,results=results,
                scope='Unpublished connection registrations with map capacity reserved before tracing. Excludes registry construction and map growth; the registry counter increases setup size. Other Rc owners remain. No timing claim.')
    options.output.parent.mkdir(parents=True,exist_ok=True)
    options.output.write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps(results,sort_keys=True))
