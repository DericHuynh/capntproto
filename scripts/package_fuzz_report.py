#!/usr/bin/env python3
"""Retain raw fuzz filenames inside a tarball accepted by upload-artifact."""
from pathlib import Path
import tarfile


SOURCES = ('target/quality/fuzz', 'target/verification/native-fuzz', 'fuzz/artifacts')


def package(root):
    root = Path(root)
    destination = root / 'target/fuzz-report.tar.gz'
    destination.parent.mkdir(parents=True, exist_ok=True)
    # Replace previous contents on reruns, even after an early setup failure.
    # No extraction or filename rewriting: AFL can replay its original corpus.
    with tarfile.open(destination, 'w:gz') as archive:
        for relative in SOURCES:
            source = root / relative
            if source.exists():
                archive.add(source, arcname=relative)
    return destination


if __name__ == '__main__':
    print(package(Path(__file__).resolve().parents[1]))
