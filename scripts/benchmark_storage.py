#!/usr/bin/env python3
"""Compare latest-value fixed-slot storage on the same filesystem, with fsync per commit.

EAE is extracted only into target/ for this experiment, not linked into the runtime.
Run without concurrent builds/tests. Results include every trial and checkpoint.
"""
import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import platform
import statistics
import subprocess
import zipfile

ROOT = Path(__file__).resolve().parent.parent
ARCHIVE_SHA = '55872653058ee56813a3417df242760b81dc002e53500ca13c7c696335cf7f5b'
CASES = [
    ('small_replace', 32, 64, 64, 100, 10, 1),
    ('4k_replace', 32, 4096, 4096, 100, 10, 1),
    ('64k_replace', 8, 65536, 65536, 100, 10, 1),
    ('1m_replace', 2, 1048576, 1048576, 20, 10, 1),
    ('4k_field', 32, 4096, 8, 100, 10, 1),
    ('64k_field', 8, 65536, 8, 100, 10, 1),
    ('sparse_64k_field', 8, 65536, 8, 100, 10, 16),
    ('snapshot_64k_field', 8, 65536, 8, 100, 1, 1),
]


def prepare_eae():
    archive = ROOT/'research/EAE-Reconstruction.zip'
    assert hashlib.sha256(archive.read_bytes()).hexdigest() == ARCHIVE_SHA
    destination = ROOT/'target/eae-benchmark'
    destination.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(archive) as source:
        for entry in source.infolist():
            target = (destination/entry.filename).resolve()
            assert target.is_relative_to(destination.resolve())
            assert (entry.external_attr >> 16) & 0o170000 != 0o120000
        source.extractall(destination)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--base-dir', type=Path, default=ROOT/'target/storage-benchmark/data')
    parser.add_argument('--report-dir', type=Path, default=ROOT/'target/storage-benchmark/reports')
    parser.add_argument('--trials', type=int, default=5)
    parser.add_argument('--iterations', type=int, default=200)
    args = parser.parse_args()
    if args.trials < 1 or args.iterations < 20:
        parser.error('trials must be positive and iterations >= 20')
    args.base_dir.mkdir(parents=True, exist_ok=True)
    args.report_dir.mkdir(parents=True, exist_ok=True)
    prepare_eae()
    manifest = ROOT/'benchmarks/storage/Cargo.toml'
    env = dict(os.environ, CARGO_TARGET_DIR=str(ROOT/'target/storage-benchmark'))
    lock = manifest.with_name('Cargo.lock')
    if not lock.exists():
        subprocess.run(['cargo', '+1.97.0', 'generate-lockfile', '--manifest-path', str(manifest), '--offline'], cwd=ROOT, env=env, check=True)
    subprocess.run(['cargo', '+1.97.0', 'auditable', 'build', '--release', '--locked', '--manifest-path', str(manifest)], cwd=ROOT, env=env, check=True)
    executable = Path(env['CARGO_TARGET_DIR'])/'release/reproto-storage-comparison'
    sources = [manifest, lock, ROOT/'benchmarks/storage/src/main.rs', ROOT/'benchmarks/storage/build.rs', ROOT/'benchmarks/storage/entry.capnp', ROOT/'rust-toolchain.toml', Path(__file__).resolve(),
               ROOT/'Cargo.toml', ROOT/'Cargo.lock', *sorted((ROOT/'src').rglob('*.rs'))]
    hashes = {str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest() for p in sources}
    rows = []
    with (args.report_dir/'runs.jsonl').open('w') as output:
        for trial in range(args.trials):
            for name, objects, entry, edit, every, snapshots, factor in CASES:
                order = ['store', 'eae'] if trial % 2 == 0 else ['eae', 'store']
                for backend in order:
                    command = [str(executable), backend, str(args.base_dir.resolve()), str(objects), str(entry), str(edit),
                               str(args.iterations), str(every), str(snapshots), str(factor)]
                    result = subprocess.run(command, cwd=ROOT, text=True, capture_output=True, check=True, timeout=120)
                    row = json.loads(result.stdout)
                    row.update(case=name, trial=trial+1, command=command)
                    rows.append(row)
                    output.write(json.dumps(row)+'\n'); output.flush()
                    print(f'{trial+1}/{args.trials} {name} {backend}: {row["commits"]["p50_ns"]/1000:.1f} us median commit', flush=True)
    summary = {}
    for name, *_ in CASES:
        summary[name] = {}
        for backend in ['store', 'eae']:
            group = [r for r in rows if r['case'] == name and r['backend'] == backend]
            summary[name][backend] = {
                'commit_p50_us': statistics.median(r['commits']['p50_ns'] / 1000 for r in group),
                'commit_p99_us': statistics.median(r['commits']['p99_ns'] / 1000 for r in group),
                'workload_ms': statistics.median(r['workload_wall_ns'] / 1e6 for r in group),
                'workload_ms_range': [min(r['workload_wall_ns']/1e6 for r in group), max(r['workload_wall_ns']/1e6 for r in group)],
                'first_snapshot_us': statistics.median(r['first_snapshots']['p50_ns'] / 1000 for r in group),
                'repeated_snapshot_us': statistics.median(r['repeated_snapshots']['p50_ns'] / 1000 for r in group),
                'reopen_with_log_ms': statistics.median(r['reopen_with_log_ns'] / 1e6 for r in group),
                'reopen_checkpoint_ms': statistics.median(r['reopen_checkpoint_ns'] / 1e6 for r in group),
                'total_written_bytes': statistics.median(r['total_written_bytes'] for r in group),
                'write_amplification': statistics.median(r['total_written_bytes']/r['logical_changed_bytes'] for r in group),
                'process_peak_rss_kib': statistics.median(r['process_peak_rss_kib'] for r in group),
            }
    assert hashes == {str(p.relative_to(ROOT)): hashlib.sha256(p.read_bytes()).hexdigest() for p in sources}, 'benchmark inputs changed during run'
    report = dict(time_utc=datetime.datetime.now(datetime.timezone.utc).isoformat(), trials=args.trials,
                  iterations=args.iterations, runs=len(rows), summary=summary, sources=hashes,
                  eae_archive_sha256=ARCHIVE_SHA, executable_sha256=hashlib.sha256(executable.read_bytes()).hexdigest(),
                  rustc=subprocess.check_output(['rustc', '+1.97.0', '--version'], text=True).strip(),
                  platform=platform.platform(), filesystem=subprocess.check_output(['stat','-f','-c','%T',str(args.base_dir)],text=True).strip(),
                  base_dir=str(args.base_dir.resolve()),
                  scope='single writer; fixed-size latest-published objects; per-object CAS metadata; durable single-object transactions; one held snapshot and 32 repeated same-generation snapshots; all scheduled plus final checkpoints accounted',
                  limits='comparison adapter, not full ORM/history/capability integration; no buffered commits; fixed slots omit allocator/graph relocation; timings from one shared host; RSS is process peak; coexistence bytes are logical, not physical peak; warm-cache reopen; fsync calls do not prove hardware power-loss behavior')
    (args.report_dir/'comparison.json').write_text(json.dumps(report,indent=2)+'\n')
    print(args.report_dir/'comparison.json')


if __name__ == '__main__':
    main()
