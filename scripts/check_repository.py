#!/usr/bin/env python3
"""Reject ignored files and large blobs in the Git index before committing."""

from pathlib import Path
import subprocess
import sys


def git(root, *args, **kwargs):
    return subprocess.check_output(["git", "-C", str(root), *args], **kwargs)


def check(root):
    errors = []
    ignored = git(root, "ls-files", "--cached", "--ignored", "--exclude-standard", "-z")
    for path in ignored.split(b"\0"):
        if path:
            errors.append(f"Ignored file is tracked: {path.decode(errors='replace')}")

    blobs = {}
    for entry in git(root, "ls-files", "--stage", "-z").split(b"\0"):
        if not entry:
            continue
        metadata, path = entry.split(b"\t", 1)
        mode, oid, stage = metadata.split()
        if stage != b"0":
            errors.append(f"Unmerged path: {path.decode(errors='replace')}")
        elif mode != b"160000":  # Submodules contain a commit, not a source blob.
            blobs.setdefault(oid, []).append(path)

    if blobs:
        sizes = git(
            root, "cat-file", "--batch-check=%(objectname) %(objecttype) %(objectsize)",
            input=b"\n".join(blobs) + b"\n",
        )
        for line in sizes.splitlines():
            oid, kind, size = line.split()
            if kind != b"blob":
                errors.append(f"Expected blob: {oid.decode()}")
            elif int(size) > 50 * 1024 * 1024:
                for path in blobs[oid]:
                    errors.append(
                        f"Blob exceeds 50 MiB: {path.decode(errors='replace')} ({int(size):,} bytes)"
                    )
    return errors


def main():
    root = Path(git(Path.cwd(), "rev-parse", "--show-toplevel").decode().strip())
    errors = check(root)
    if errors:
        print("Repository check failed:", file=sys.stderr)
        for error in errors[:20]:
            print(f"  {error}", file=sys.stderr)
        if len(errors) > 20:
            print(f"  ... and {len(errors) - 20} more", file=sys.stderr)
        return 1
    print("Repository check passed: no tracked ignored files or blobs over 50 MiB.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
