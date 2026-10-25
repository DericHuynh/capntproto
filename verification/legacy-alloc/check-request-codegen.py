#!/usr/bin/env python3
"""Compile isolated request callers to diagnose aggregate copies; no timings."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--baseline', required=True)
parser.add_argument('--output', type=Path, required=True)
options = parser.parse_args()
if not re.fullmatch(r'[0-9a-f]{40}', options.baseline):
    raise SystemExit('baseline must be a full commit SHA')
repo = Path(__file__).resolve().parents[2]
subject = 'crates/capntproto-core/src/capability.rs'
baseline = subprocess.check_output(['git', 'show', f'{options.baseline}:{subject}'], cwd=repo)
candidate = (repo / subject).read_bytes()
options.output.parent.mkdir(parents=True, exist_ok=True)
reference = '''
// Diagnostic-only reference in the isolated snapshot, never the real checkout.
#[cfg(feature = "alloc")]
impl<P,R> Request<P,R>
where R:Pipelined+Owned+'static+Unpin, R::Pipeline:FromTypelessPipeline {
 pub fn diagnostic_original_send(self)->RemotePromise<R> {
  let RemotePromise{promise,pipeline,..}=self.hook.send();
  let typed_promise=promise.into_typed_response();
  RemotePromise{promise:typed_promise,pipeline:FromTypelessPipeline::new(pipeline)}
 }
}
'''
probe = '''extern crate capnp;
use capnp::capability::Request;
use capnp::schema_capnp::node::Owned;
use core::{future::Future,pin::Pin};
type Call=Request<Owned,Owned>;
#[unsafe(no_mangle)]
pub fn original_future(request:Call)->Pin<Box<dyn Future<Output=Result<u64,capnp::Error>>>>{
 Box::pin(async move {let response=request.diagnostic_original_send().promise.await?;Ok(response.get()?.get_id())})
}
#[unsafe(no_mangle)]
pub fn current_future(request:Call)->Pin<Box<dyn Future<Output=Result<u64,capnp::Error>>>>{
 Box::pin(async move {let response=request.send().promise.await?;Ok(response.get()?.get_id())})
}
'''

def callers(assembly):
    functions, name = {}, None
    for line in assembly.splitlines():
        if line.startswith('\t.type\t'):
            name = line.split('\t')[2].removesuffix(',@function')
            functions[name] = []
        elif line.startswith('\t.size\t'):
            name = None
        elif name and line.startswith('\t') and not line.lstrip().startswith(('.', '#')):
            functions[name].append(line.strip())
    selected = {}
    for kind in ['original', 'current']:
        matches = [instructions for name, instructions in functions.items()
                   if f'::{kind}_future::{{closure#0}}' in name and 'drop_glue' not in name]
        if len(matches) != 1:
            raise SystemExit(f'expected one {kind} caller, found {len(matches)}')
        selected[kind] = matches[0]
    return selected


def normalize(instructions):
    # Two separate source functions have different local labels and panic
    # source-location constants. Preserve all actual instruction/stack forms.
    result = []
    for line in instructions:
        line = re.sub(r'\.L(BB|JTI)\d+_', r'.L\1_', line)
        line = re.sub(r'\.Lanon\.[a-f0-9]+\.\d+\(%rip\)', '<source-constant>(%rip)', line)
        result.append(line)
    return result

results = {}
with tempfile.TemporaryDirectory(prefix='legacy-request-codegen-') as directory:
    root = Path(directory)
    source = root / 'source'
    source.mkdir()
    shutil.copytree(repo / 'crates/capntproto-core/src', source / 'src')
    manifest = (repo / 'crates/capntproto-core/Cargo.toml').read_text()
    (source / 'Cargo.toml').write_text(manifest + '\n[workspace]\n')
    shutil.copy2(repo / 'Cargo.lock', source / 'Cargo.lock')
    harness = root / 'probe.rs'
    harness.write_text(probe)
    target = root / 'target'
    env = os.environ.copy()
    env.update(CARGO_TARGET_DIR=str(target), CARGO_BUILD_JOBS='1', CARGO_INCREMENTAL='0',
               CARGO_PROFILE_RELEASE_DEBUG='1', CARGO_PROFILE_RELEASE_OVERFLOW_CHECKS='true')
    setup = subprocess.run(['cargo', 'generate-lockfile', '--offline', '--manifest-path', str(source / 'Cargo.toml')], cwd=repo, env=env, text=True, capture_output=True)
    if setup.returncode:
        raise SystemExit(setup.stdout + setup.stderr)
    lock_sha256 = hashlib.sha256((source / 'Cargo.lock').read_bytes()).hexdigest()
    for label, text in [('baseline', baseline), ('candidate', candidate)]:
        (source / 'src/capability.rs').write_text(text.decode() + reference)
        log = options.output.parent / f'request-codegen-{label}.log'
        with log.open('w') as output:
            run = subprocess.run(['cargo', 'build', '--locked', '--manifest-path', str(source / 'Cargo.toml'),
                            '--release', '--no-default-features', '--features', 'alloc'],
                           cwd=repo, env=env, stdout=output, stderr=subprocess.STDOUT)
        if run.returncode:
            raise SystemExit(log.read_text())
        assembly = root / 'probe.s'
        subprocess.run(['rustc', '--edition=2021', '--crate-type', 'lib', '--emit=asm',
                        '-C', 'opt-level=3', '-C', 'overflow-checks=on', '-C', 'debuginfo=1',
                        '-L', f'dependency={target}/release/deps', '--extern', f'capnp={target}/release/libcapnp.rlib',
                        str(harness), '-o', str(assembly)], check=True)
        demangled = subprocess.check_output(['c++filt', '--format=rust'], input=assembly.read_bytes()).decode()
        evidence = options.output.parent / f'request-codegen-{label}.assembly'
        evidence.write_text(demangled)
        functions = callers(demangled)
        results[label] = dict(instructions={k: len(v) for k, v in functions.items()},
                              normalized_equal=normalize(functions['original']) == normalize(functions['current']),
                              assembly_sha256=hashlib.sha256(evidence.read_bytes()).hexdigest())
report = dict(baseline=options.baseline, subject=subject,
              source_sha256=dict(baseline=hashlib.sha256(baseline).hexdigest(), candidate=hashlib.sha256(candidate).hexdigest()),
              rustc=subprocess.check_output(['rustc', '--version'], text=True).strip(),
              target='host compiler default', optimization='opt-level=3, overflow checks on, debug=1',
              probe_sha256=hashlib.sha256(probe.encode()).hexdigest(),
              reference_sha256=hashlib.sha256(reference.encode()).hexdigest(), lock_sha256=lock_sha256, results=results,
              scope='Static caller instruction forms, not executed; isolated core source at the same path. Reference uses pre-refactor send operations. Other core files are held current. Normalization removes local function label numbers and source-location constants; not byte equivalence or whole RPC/native performance.')
options.output.write_text(json.dumps(report, indent=2) + '\n')
print(json.dumps(results, sort_keys=True))
