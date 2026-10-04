#!/usr/bin/env python3
"""Reject new unsafe lint debt; retain an explicit, source-bound core-runtime baseline."""
import hashlib
import json
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]
BASELINE = ROOT / 'quality/unsafe-baseline.json'
LINTS = ('clippy::undocumented_unsafe_blocks', 'clippy::missing_safety_doc',
         'unsafe_op_in_unsafe_fn')


def inventory(lines, root):
    diagnostics = set()
    for line in lines:
        if not line.startswith('{'):
            continue
        event = json.loads(line)
        if event.get('reason') != 'compiler-message':
            continue
        message = event['message']
        code = (message.get('code') or {}).get('code')
        if code == 'E0133':
            code = 'unsafe_op_in_unsafe_fn'
        if code not in LINTS:
            continue
        spans = [s for s in message['spans'] if s['is_primary']]
        if not spans:
            raise ValueError('unsafe diagnostic without a source span')
        for span in spans:
            path = (root / span['file_name']).resolve().relative_to(root.resolve()).as_posix()
            diagnostics.add((path, code, span['line_start'], span['column_start']))
    files = {}
    for path, code, line, column in sorted(diagnostics):
        item = files.setdefault(path, {'sha256': hashlib.sha256((root / path).read_bytes()).hexdigest(),
                                       'diagnostics': {}})
        item['diagnostics'][code] = item['diagnostics'].get(code, 0) + 1
    return files


def check(actual, baseline):
    expected = baseline['files']
    # Exceptions can only name core runtime implementation files. They are
    # debt, not safety approval, and cannot expand to application/transport code.
    if any(not path.startswith('crates/capntproto-core/src/') for path in expected):
        raise ValueError('unsafe exceptions must stay inside the imported core runtime')
    return sorted(path for path in actual.keys() | expected.keys()
                  if actual.get(path) != expected.get(path))


def main():
    command = ['cargo', 'clippy', '--locked', '--workspace', '--all-targets',
               '--message-format=json', '--', '-D', 'warnings',
               *['--force-warn=' + lint for lint in LINTS]]
    result = subprocess.run(command, cwd=ROOT, stdout=subprocess.PIPE, text=True)
    output = ROOT / 'target/quality/unsafe'
    output.mkdir(parents=True, exist_ok=True)
    (output / 'clippy.jsonl').write_text(result.stdout)
    if result.returncode:
        for line in result.stdout.splitlines():
            if line.startswith('{'):
                event = json.loads(line)
                message = event.get('message', {})
                if message.get('level') == 'error':
                    print(message.get('rendered', message), file=sys.stderr)
        return result.returncode
    actual = inventory(result.stdout.splitlines(), ROOT)
    (output / 'inventory.json').write_text(json.dumps(actual, indent=2) + '\n')
    changed = check(actual, json.loads(BASELINE.read_text()))
    if changed:
        print('Unsafe documentation changed; document new code and review pre-existing exceptions:',
              *changed, sep='\n', file=sys.stderr)
        return 1
    count = sum(sum(item['diagnostics'].values()) for item in actual.values())
    print(f'No new unsafe lint debt. {count} pre-existing diagnostics remain explicitly tracked.')
    return 0


if __name__ == '__main__':
    sys.exit(main())
