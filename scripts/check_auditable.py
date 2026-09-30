#!/usr/bin/env python3
"""Require embedded cargo-auditable metadata; retain binary hashes and inventories."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess


def inspect(binary: Path) -> dict:
    result = subprocess.run(
        ["rust-audit-info", str(binary)], capture_output=True, text=True
    )
    if result.returncode:
        raise ValueError(f"{binary}: {result.stderr.strip()}")
    metadata = json.loads(result.stdout)
    packages = metadata.get("packages", [])
    if not packages or not any(package.get("root") is True for package in packages):
        raise ValueError(f"{binary}: missing root dependency inventory")
    return {"sha256": hashlib.sha256(binary.read_bytes()).hexdigest(), "metadata": metadata}


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", required=True, type=Path)
    parser.add_argument("binaries", nargs="+", type=Path)
    args = parser.parse_args()
    # A failed inspection must never leave a previous passing report behind.
    args.output.unlink(missing_ok=True)
    inventories = {}
    for binary in args.binaries:
        if binary.name in inventories:
            raise ValueError(f"duplicate binary name: {binary.name}")
        inventories[binary.name] = inspect(binary)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(inventories, indent=2) + "\n", encoding="utf-8")
    print(f"Verified embedded dependency metadata in {len(inventories)} binaries")


if __name__ == "__main__":
    main()
