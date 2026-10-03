#!/usr/bin/env python3
"""Check project-owned documentation and export docs/wiki for GitHub Wiki.

Standard library only. This intentionally never invokes Git or a network client.
The supported Markdown subset is inline links/images and reference definitions;
code fences, inline code and HTML comments are not treated as links.
"""
from __future__ import annotations

import argparse
import hashlib
import html
import json
import re
import sys
from pathlib import Path
from urllib.parse import quote, unquote, urlsplit

ROOT = Path(__file__).resolve().parents[1]
WIKI = Path('docs/wiki')
INVENTORY = Path('docs/documentation-review.json')
MANIFEST = 'wiki-export.json'
FORMAT = 'capntproto-wiki-export-v1'
# Build outputs and the optional C++ submodule need not exist in a fresh clone.
OPTIONAL_PREFIXES = ('target/', 'dist/', 'vendor/capnproto/')
LINK = re.compile(r'!?\[[^\]\n]*\]\(\s*(?P<inline><[^>\n]+>|(?:[^\s()\\]|\\.|\([^()\n]*\))+)'
                  r'(?:\s+["\'][^\n]*?["\'])?\s*\)'
                  r'|^ {0,3}\[[^\]\n]+\]:\s*(?P<reference><[^>\n]+>|\S+)', re.M)


def prose(text: str, *, inline_code: bool = True) -> str:
    """Mask non-prose without changing offsets, for safe URL replacement."""
    lines = text.splitlines(keepends=True)
    fence = None
    for i, line in enumerate(lines):
        marker = re.match(r'^ {0,3}(`{3,}|~{3,})', line)
        if fence:
            lines[i] = re.sub(r'[^\n]', ' ', line)
            if re.match(r'^ {0,3}' + re.escape(fence[0]) + '{' + str(len(fence)) + r',}\s*$', line):
                fence = None
        elif marker:
            fence = marker[1]
            lines[i] = re.sub(r'[^\n]', ' ', line)
        elif line.startswith('    ') or line.startswith('\t'):
            lines[i] = re.sub(r'[^\n]', ' ', line)
    result = ''.join(lines)
    result = re.sub(r'<!--[\s\S]*?-->', lambda m: re.sub(r'[^\n]', ' ', m[0]), result)
    if inline_code:
        result = re.sub(r'(`+)([^`]|(?!\1)`)*?\1', lambda m: ' ' * len(m[0]), result)
    return result


def links(text: str):
    for match in LINK.finditer(prose(text)):
        group = 'inline' if match['inline'] is not None else 'reference'
        start, end = match.span(group)
        if text[start] == '<':
            start, end = start + 1, end - 1
        yield start, end, text[start:end]


def rewrite_links(text: str, transform) -> str:
    for start, end, destination in reversed(list(links(text))):
        text = text[:start] + transform(destination) + text[end:]
    return text


def anchors(text: str) -> set[str]:
    """GitHub heading slugs, including duplicate headings and explicit anchors."""
    visible = prose(text, inline_code=False)
    result = set(re.findall(r'<a\s+[^>]*?(?:id|name)=["\']([^"\']+)', visible))
    used: set[str] = set()
    headings = []
    lines = visible.splitlines()
    for i, line in enumerate(lines):
        match = re.match(r'^ {0,3}#{1,6}\s+(.+?)\s*#*\s*$', line)
        if match:
            headings.append(match[1])
        elif i and re.fullmatch(r' {0,3}(?:=+|-+)\s*', line) and lines[i - 1].strip():
            headings.append(lines[i - 1].strip())
    for heading in headings:
        heading = re.sub(r'!?\[([^]]*)\]\([^)]*\)', r'\1', heading)
        heading = html.unescape(re.sub(r'<[^>]*>', '', heading)).lower()
        slug = re.sub(r'[^\w\-\s]', '', heading).replace(' ', '-').replace('\t', '-')
        unique = slug
        suffix = 0
        while unique in used:
            suffix += 1
            unique = f'{slug}-{suffix}'
        used.add(unique)
        result.add(unique)
    return result


def local_target(root: Path, source: Path, destination: str):
    url = urlsplit(html.unescape(destination))
    if url.scheme or url.netloc:
        return None
    if not url.path:
        target = source
    else:
        target = source.parent / unquote(url.path)
    target = target.resolve()
    if not target.is_relative_to(root.resolve()):
        raise ValueError(f'link escapes repository: {destination}')
    return target, unquote(url.fragment)


def owned_documents(root: Path) -> list[Path]:
    paths = set(root.glob('*.md'))
    for directory in ('docs', '.github', 'crates', 'benchmarks'):
        paths.update(p for p in (root / directory).rglob('*.md')
                     if not any(part in ('target', 'node_modules') for part in p.relative_to(root).parts))
    paths.update(p for p in (root / 'research/reports').glob('*.md'))
    paths.update((root / 'vendor').glob('*/REPROTO.md'))
    return sorted(p.relative_to(root) for p in paths)


def check_links(root: Path, documents: list[Path]) -> list[str]:
    errors = []
    cache = {}
    for relative in documents:
        source = root / relative
        if relative == Path('docs/README.template.md'):
            # Rendered at repository root; its generated output is checked.
            continue
        if not source.is_file():
            errors.append(f'{relative}: missing document')
            continue
        body = source.read_text()
        for start, _, destination in links(body):
            location = f'{relative}:{body[:start].count(chr(10)) + 1}'
            try:
                resolved = local_target(root, source, destination)
            except ValueError as exc:
                errors.append(f'{location}: {exc}')
                continue
            if resolved is None:
                continue
            target, fragment = resolved
            repo_path = target.relative_to(root.resolve()).as_posix()
            if not target.exists():
                if not repo_path.startswith(OPTIONAL_PREFIXES):
                    errors.append(f'{location}: missing target {destination}')
                continue
            if fragment and target.suffix.lower() == '.md':
                if target not in cache:
                    cache[target] = anchors(target.read_text())
                if fragment not in cache[target]:
                    errors.append(f'{location}: missing anchor {destination}')
    return errors


def check(root: Path) -> list[str]:
    documents = owned_documents(root)
    errors = check_links(root, documents)
    pages = sorted((root / WIKI).glob('*.md'))
    seen = set()
    for page in pages:
        key = page.stem.casefold()
        if key in seen:
            errors.append(f'duplicate wiki page name: {page.name}')
        seen.add(key)
    for required in ('Home.md', '_Sidebar.md', '_Footer.md'):
        if not (root / WIKI / required).is_file():
            errors.append(f'missing wiki navigation file: {required}')
    sidebar = root / WIKI / '_Sidebar.md'
    if sidebar.exists():
        linked = set()
        for _, _, destination in links(sidebar.read_text()):
            try:
                target = local_target(root, sidebar, destination)
                if target:
                    linked.add(target[0])
            except ValueError:
                pass  # Already reported by check_links.
        for page in pages:
            if not page.name.startswith('_') and page.resolve() not in linked:
                errors.append(f'wiki page absent from sidebar: {page.name}')
    try:
        inventory = json.loads((root / INVENTORY).read_text())
        registered = inventory['documents']
        if len(registered) != len(set(registered)):
            errors.append('documentation inventory contains duplicate paths')
        actual = {p.as_posix() for p in documents}
        for path in sorted(actual - set(registered)):
            errors.append(f'unregistered document: {path}')
        for path in sorted(set(registered) - actual):
            errors.append(f'inventory document missing: {path}')
        for entry in inventory['migrations']:
            for destination in entry['destinations']:
                if destination not in actual:
                    errors.append(f'migration destination missing: {destination}')
    except (OSError, ValueError, KeyError, TypeError) as exc:
        errors.append(f'invalid documentation inventory: {exc}')
    return errors


def export_url(root: Path, source: Path, destination: str, repository: str, source_ref: str) -> str:
    resolved = local_target(root, source, destination)
    if resolved is None or destination.startswith('#'):
        return destination
    target, _ = resolved
    url = urlsplit(destination)
    relative = target.relative_to(root.resolve())
    base = f'https://github.com/{repository}'
    if relative.parent == WIKI and relative.suffix == '.md':
        result = f'{base}/wiki/{quote(relative.stem)}'
    else:
        kind = 'tree' if target.is_dir() else 'blob'
        result = f'{base}/{kind}/{quote(source_ref, safe="")}/{quote(relative.as_posix())}'
    if url.query:
        result += '?' + url.query
    if url.fragment:
        result += '#' + url.fragment
    return result


def build(root: Path, output: Path, repository: str, source_ref: str) -> int:
    if not re.fullmatch(r'[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+', repository):
        raise ValueError('repository must be owner/name')
    if not source_ref or any(c.isspace() or ord(c) < 32 for c in source_ref):
        raise ValueError('source ref must be nonempty and contain no whitespace')
    errors = check(root)
    if errors:
        raise ValueError('\n'.join(errors))
    output = output.absolute()
    # Never write through symlinks, into the source tree, or over unrelated files.
    if any(p.is_symlink() for p in (output, *output.parents)):
        raise ValueError('output path must not contain symlinks')
    protected = [(root / p).resolve() for p in owned_documents(root)]
    if any(p.is_relative_to(output.resolve()) for p in protected):
        raise ValueError('output must not contain source documents')
    previous = {}
    if output.exists():
        manifest = output / MANIFEST
        if any(output.iterdir()):
            if manifest.is_symlink() or not manifest.is_file():
                raise ValueError('nonempty output is not an owned wiki export')
            previous = json.loads(manifest.read_text())
            if previous.get('format') != FORMAT or not isinstance(previous.get('pages'), dict):
                raise ValueError('unrecognized wiki export manifest')
            for name, digest in previous['pages'].items():
                if Path(name).name != name or not name.endswith('.md'):
                    raise ValueError('invalid page name in export manifest')
                page = output / name
                if page.is_symlink() or not page.is_file() or hashlib.sha256(page.read_bytes()).hexdigest() != digest:
                    raise ValueError(f'previous export changed: {name}; use a fresh output directory')
            if {p.name for p in output.iterdir()} != set(previous['pages']) | {MANIFEST}:
                raise ValueError('output contains unrelated files; use a fresh output directory')
    rendered = {}
    for source in sorted((root / WIKI).glob('*.md')):
        rendered[source.name] = rewrite_links(source.read_text(), lambda url: export_url(
            root, source, url, repository, source_ref)).encode()
    metadata = {'format': FORMAT, 'repository': repository, 'source_ref': source_ref,
                'pages': {name: hashlib.sha256(data).hexdigest() for name, data in rendered.items()}}
    output.mkdir(parents=True, exist_ok=True)
    for name, data in rendered.items():
        (output / name).write_bytes(data)
    for name in previous.get('pages', {}).keys() - rendered.keys():
        (output / name).unlink()
    (output / MANIFEST).write_text(json.dumps(metadata, indent=2) + '\n')
    return len(rendered)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest='command', required=True)
    sub.add_parser('check', help='validate local links, anchors, navigation and inventory')
    export = sub.add_parser('build', help='write a validated GitHub Wiki export')
    export.add_argument('--output', type=Path, default=ROOT / 'target/wiki')
    export.add_argument('--repository', default='DericHuynh/capntproto')
    export.add_argument('--source-ref', default='main')
    args = parser.parse_args()
    try:
        if args.command == 'check':
            errors = check(ROOT)
            if errors:
                raise ValueError('\n'.join(errors))
            print(f'Validated {len(owned_documents(ROOT))} project documents.')
        else:
            count = build(ROOT, args.output, args.repository, args.source_ref)
            print(f'Exported {count} wiki files to {args.output}. No publication performed.')
    except (OSError, ValueError, TypeError) as exc:
        print(str(exc), file=sys.stderr)
        return 1
    return 0


if __name__ == '__main__':
    sys.exit(main())
