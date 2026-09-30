#!/usr/bin/env python3
"""Independent conformance audit; exit 1 means a requirement is unsatisfied.

Runs unchanged models in an isolated directory. Does not overwrite the
historical reports/checks.json or count failed requirements as passing tests.
"""
import argparse
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
CASES = [
    ("AutomaticHandoff", "requirement", "LiveSpec", "AutomaticHandoffPlan",
     "PROPERTIES\n AutomaticHandoff", 3, 4, 8),
    ("AutomaticHandoffSettles", "requirement", "LiveSpec", "AutomaticHandoffPlan",
     "INVARIANTS\n ReferenceConservation\n VinesHeldUntilAcceptance\nPROPERTIES\n AutomaticHandoffSettles", 3, 4, 8),
    ("ProviderCapabilitySurvives", "requirement", "LiveSpec", "ProviderLossHandoffPlan",
     "PROPERTIES\n ProviderCapabilitySurvives", 3, 3, 7),
    ("ScriptedHandoff", "positive_control", "LiveSpec", "ScriptedHandoffPlan",
     "PROPERTIES\n ScriptedHandoff", 3, 4, 8),
    ("GeneratedReleaseEffect", "positive_control", "LiveSpec", "ExplicitReleasePlan",
     "INVARIANTS\n GeneratedReleaseEffect", 2, 3, 4),
    ("WireReleaseEffect", "abstraction_diagnostic", "UnitSpec", "BasicPlan",
     "INVARIANTS\n WireReleaseEffect", 3, 4, 8),
    ("WireRequestMethod", "abstraction_diagnostic", "UnitSpec", "BasicPlan",
     "INVARIANTS\n WireRequestMethod", 3, 4, 8),
    ("EqualJoin", "positive_control", "LiveSpec", "LocalJoinPlan",
     "INVARIANTS\n JoinAgreement\n JoinResources\nPROPERTIES\n PlanCompletes\n EqualJoinOutcome", 2, 4, 8),
    ("UnequalJoin", "positive_control", "LiveSpec", "UnequalJoinPlan",
     "INVARIANTS\n JoinAgreement\n JoinResources\nPROPERTIES\n PlanCompletes\n UnequalJoinOutcome", 2, 4, 8),
]


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--java", default="java")
    parser.add_argument("--jar", type=Path, required=True)
    parser.add_argument("--timeout", type=int, default=120)
    args = parser.parse_args()
    started = datetime.now(timezone.utc).isoformat()
    java_version = subprocess.check_output([args.java, "-version"],
                                           stderr=subprocess.STDOUT, text=True)
    sources = [ROOT / name for name in ("CapnpNetwork.tla", "CapnpNetworkChecks.tla")]
    sources += [Path(__file__).with_name("CapnpConformance.tla"), Path(__file__).resolve()]
    hashes = {str(p.relative_to(ROOT)): digest(p) for p in sources}
    output = ROOT / "reports/conformance"
    output.mkdir(parents=True, exist_ok=True)
    results = []
    with tempfile.TemporaryDirectory(prefix="capnp-conformance-") as directory:
        work = Path(directory)
        for source in sources:
            if source.suffix == ".tla":
                shutil.copyfile(source, work / source.name)
        for name, kind, spec, plan, assertions, vats, ids, ops in CASES:
            config = (f"SPECIFICATION {spec}\nCONSTANTS\n VatCount = {vats}\n"
                      f" IdCount = {ids}\n MaxOps = {ops}\n Plan <- {plan}\n"
                      ' Bug = "none"\n AllowLoss = FALSE\n Introductions = TRUE\n'
                      f" ExpectSuccess = TRUE\nCHECK_DEADLOCK FALSE\n{assertions}\n")
            (work / f"{name}.cfg").write_text(config)
            (output / f"{name}.cfg").write_text(config)
            command = [args.java, "-Xmx1g", "-cp", str(args.jar.resolve()),
                       "tlc2.TLC", "-workers", "1", "-fp", "0", "-metadir",
                       str(work / name), "-config", f"{name}.cfg", "CapnpConformance"]
            try:
                run = subprocess.run(command, cwd=work, text=True,
                                     stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                                     timeout=args.timeout)
                log, code = run.stdout, run.returncode
            except subprocess.TimeoutExpired as exc:
                log, code = exc.stdout or b"", None
                if isinstance(log, bytes):
                    log = log.decode(errors="replace")
            (output / f"{name}.log").write_text(log)
            if code == 0 and "Model checking completed. No error has been found." in log:
                status = "satisfied"
            elif (code == 12 and f"Invariant {name} is violated" in log) or (
                    code == 13 and "Temporal properties were violated" in log):
                status = "unsatisfied"
            else:
                status = "checker_error"
            results.append({"name": name, "kind": kind, "status": status,
                            "exit_code": code, "config_sha256": digest(output / f"{name}.cfg")})
            print(f"{name}: {status} ({kind}, TLC exit {code})", flush=True)
    unchanged = hashes == {str(p.relative_to(ROOT)): digest(p) for p in sources}
    report = {"started_utc": started, "finished_utc": datetime.now(timezone.utc).isoformat(),
              "java_version": java_version,
              "schema_revision": json.loads((ROOT / "upstream/revision.json").read_text()),
              "sources": hashes, "sources_unchanged": unchanged,
              "jar_sha256": digest(args.jar), "cases": results,
              "note": "Abstraction diagnostics are not reachable protocol counterexamples."}
    (output / "audit.json").write_text(json.dumps(report, indent=2) + "\n")
    return 0 if unchanged and all(r["status"] == "satisfied" for r in results) else 1


if __name__ == "__main__":
    raise SystemExit(main())
