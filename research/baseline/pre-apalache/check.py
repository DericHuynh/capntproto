#!/usr/bin/env python3
"""Run bounded TLC checks, negative controls, and reachability witnesses."""
import argparse
from datetime import datetime, timezone
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parent
CASES = {
    "Bilateral": None,
    "Reuse": None,
    "Pipeline": None,
    "Liveness": None,
    "Chain": None,
    "Fanout": None,
    "BugReuse": "NoPrematureReuse",
    "BugCancellation": "InterestedChildPreserved",
    "BugOrder": "EOrder",
    "WitnessPipeline": "NoEarlyPipelineWitness",
    "WitnessReuse": "NoReuseWitness",
    "WitnessCrossing": "NoCrossedFinishWitness",
}


MODULES = {}
for config in sorted((ROOT / "configs").glob("*.cfg")):
    source = config.read_text()
    module = re.search(r"@module: (\w+)", source)
    violation = re.search(r"@violation: (\w+)", source)
    MODULES[config.stem] = module[1] if module else "CapnpRpc"
    if config.stem not in CASES:
        CASES[config.stem] = violation[1] if violation else None


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--list", action="store_true", help="List configurations and their modules")
    parser.add_argument("cases", nargs="*", help="Default: all checked configurations")
    parser.add_argument("--jar", default=os.environ.get("TLA2TOOLS_JAR", str(ROOT / "tools/tla2tools.jar")))
    parser.add_argument("--java", default=os.environ.get("JAVA", "java"))
    parser.add_argument("--timeout", type=int, default=600)
    parser.add_argument("--workers", type=int, default=2)
    parser.add_argument("--heap", default="1g", help="Maximum Java heap, e.g. 1g or 4g")
    parser.add_argument("--reports-dir", type=Path, default=ROOT / "reports")
    args = parser.parse_args()
    if args.list:
        for name in CASES:
            print(f"{name}: {MODULES[name]}" + (f" (expects {CASES[name]})" if CASES[name] else ""))
        return 0
    if args.workers < 1 or args.timeout < 1:
        parser.error("workers and timeout must be positive")
    if not re.fullmatch(r"[1-9][0-9]*[mMgG]", args.heap):
        parser.error("heap must be a positive integer followed by m or g")
    jar = Path(args.jar).expanduser().resolve()
    if not jar.is_file():
        parser.error("TLC jar missing; pass --jar PATH or set TLA2TOOLS_JAR (see README).")
    cases = args.cases or list(CASES)
    unknown = set(cases) - CASES.keys()
    if unknown:
        parser.error("Unknown cases: " + ", ".join(sorted(unknown)))
    started = datetime.now(timezone.utc).isoformat()
    source_hashes = {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in ROOT.glob("*.tla")}
    runner_hash = hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
    config_hashes = {n: hashlib.sha256((ROOT / "configs" / (n + ".cfg")).read_bytes()).hexdigest() for n in cases}
    java_version = subprocess.check_output([args.java, "-version"], stderr=subprocess.STDOUT, text=True).strip()
    reports = args.reports_dir.resolve()
    reports.mkdir(parents=True, exist_ok=True)
    results = []
    for name in cases:
        expected = CASES[name]
        case_started = time.monotonic()
        with tempfile.TemporaryDirectory(prefix="capnp-tlc-") as temp:
            command = [args.java, "-XX:-UsePerfData", "-XX:+UseParallelGC", "-Xmx" + args.heap, "-cp", str(jar),
                       "tlc2.TLC", "-workers", str(args.workers), "-fp", "0",
                       "-config", f"configs/{name}.cfg", "-metadir", temp, MODULES[name]]
            try:
                run = subprocess.run(command, cwd=ROOT, text=True,
                                     stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                                     timeout=args.timeout)
                output, code = run.stdout, run.returncode
            except subprocess.TimeoutExpired as exc:
                output = exc.stdout or b""
                if isinstance(output, bytes):
                    output = output.decode(errors="replace")
                output += "\nRUNNER TIMEOUT: check incomplete.\n"
                code = -1
            (reports / f"{name}.log").write_text(output)
            violation = re.search(r"Invariant (\w+) is violated", output)
            actual = violation.group(1) if violation else None
            ok = ((code == 0 and "Model checking completed. No error has been found." in output)
                  if expected is None else (code == 12 and actual == expected))
            counts = re.search(r"([\d,]+) states generated, ([\d,]+) distinct states found", output)
            depth = re.search(r"depth of the complete state graph search is (\d+)", output)
            record = {"case": name, "module": MODULES[name], "expected_violation": expected,
                      "actual_violation": actual, "exit_code": code, "passed": ok,
                      "generated_states": int(counts[1].replace(",", "")) if counts else None,
                      "distinct_states": int(counts[2].replace(",", "")) if counts else None,
                      "search_depth": int(depth[1]) if depth else None,
                      "elapsed_seconds": round(time.monotonic() - case_started, 3)}
            results.append(record)
            label = "PASS" if ok else "FAIL"
            detail = f"expected counterexample: {expected}" if expected else "bounded verification"
            print(f"{label} {name}: {detail}; {record['distinct_states']} distinct states", flush=True)
            if not ok:
                print(output[-3500:], file=sys.stderr)
    report = {"started_utc": started, "finished_utc": datetime.now(timezone.utc).isoformat(),
              "runner_sha256": runner_hash,
              "schema_revision": json.loads((ROOT / "upstream/revision.json").read_text()),
              "workers": args.workers, "timeout_seconds": args.timeout, "heap": args.heap,
              "model_sha256": source_hashes, "java_version": java_version,
              "tlc_jar_sha256": hashlib.sha256(jar.read_bytes()).hexdigest(),
              "config_sha256": config_hashes,
              "results": results}
    unchanged = (runner_hash == hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
                 and source_hashes == {p.name: hashlib.sha256(p.read_bytes()).hexdigest() for p in ROOT.glob("*.tla")}
                 and config_hashes == {n: hashlib.sha256((ROOT / "configs" / (n + ".cfg")).read_bytes()).hexdigest() for n in cases})
    report["sources_unchanged_during_run"] = unchanged
    if not unchanged:
        print("FAIL: model/configuration changed during checking; rerun before claiming validation.", file=sys.stderr)
    (reports / "checks.json").write_text(json.dumps(report, indent=2) + "\n")
    return 0 if unchanged and all(r["passed"] for r in results) else 1


if __name__ == "__main__":
    sys.exit(main())
