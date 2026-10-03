#!/usr/bin/env python3
"""Collect CI data, generate README/charts, or publish trusted default-branch data."""
import argparse
import json
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'quality'))
from reporting.data import collect  # noqa: E402


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest='command', required=True)
    command = commands.add_parser('collect')
    command.add_argument('--input', type=Path, required=True)
    command.add_argument('--kind', choices=['full', 'benchmark'], required=True)
    command.add_argument('--output', type=Path, required=True)
    command.add_argument('--failures-output', type=Path)
    command = commands.add_parser('render')
    command.add_argument('--root', type=Path, default=Path(__file__).resolve().parents[1])
    command.add_argument('--output', type=Path)
    command = commands.add_parser('publish')
    command.add_argument('--event', type=Path, required=True)
    args = parser.parse_args()
    if args.command == 'collect':
        # Invalidate an earlier local success before parsing any current evidence.
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.unlink(missing_ok=True)
        if args.failures_output:
            args.failures_output.unlink(missing_ok=True)
        value = collect(args.input, args.kind)
        args.output.write_text(json.dumps(value, indent=2) + '\n')
        if args.failures_output:
            from reporting.render import failure_details
            args.failures_output.parent.mkdir(parents=True, exist_ok=True)
            args.failures_output.write_text('# Failed workspace tests\n\n' + failure_details(value))
    elif args.command == 'render':
        from reporting.render import render
        history = json.loads((args.root / 'docs/reports/history.json').read_text())
        render((args.root / 'docs/README.template.md').read_text(), history, args.output or args.root)
    else:
        from reporting.publish import publish
        publish(args.event)


if __name__ == '__main__':
    main()
