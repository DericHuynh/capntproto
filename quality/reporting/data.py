"""Read only validated CI evidence; preserve failures and unknown measurements."""
import hashlib
import json
import math
import os
from pathlib import Path
import re

FORMAT = 1
CHARTS = {
    'latency-p50', 'latency-p95', 'latency-p99', 'request-rate',
    'latency-difference', 'request-rate-difference', 'instruction-counts',
    'coverage-lines', 'coverage-regions', 'coverage-functions', 'coverage-branches',
}
COUNTS = ('passed', 'failed', 'errors', 'skipped')


def integer(value):
    if type(value) is not int or not 0 <= value <= 10_000_000:
        raise ValueError('invalid test counter')
    return value


def test_counts(log, command_passed):
    """Count outer libtest events, never nested test output or benchmark iterations.

    Unfinished suites charge announced tests without terminal events to errors.
    A build failure has unknown test totals, not an invented failed test.
    """
    totals = dict.fromkeys(COUNTS, 0)
    suites = 0
    unfinished = False
    active = None
    failures = []
    label = ''
    output_budget = 256 * 1024

    def close(incomplete):
        nonlocal active, suites, unfinished
        if active is None:
            return
        remaining = active['announced'] - sum(active['counts'].values())
        if remaining < 0 or (not incomplete and remaining):
            raise ValueError('libtest counters disagree with announced tests')
        active['counts']['errors'] += remaining
        for key in COUNTS:
            totals[key] += active['counts'][key]
        unfinished |= incomplete
        suites += 1
        active = None

    for line in log.splitlines():
        if not line.startswith('{'):
            match = re.match(r'^\s*(Running .+|Doc-tests .+)$', line)
            if match:
                label = match[1][:1024]
            continue
        try:
            item = json.loads(line)
        except json.JSONDecodeError:
            # A truncated JSON event cannot masquerade as a complete result.
            if active is not None:
                unfinished = True
            continue
        if not isinstance(item, dict):
            continue
        kind, event = item.get('type'), item.get('event')
        if kind == 'suite' and event == 'started':
            close(True)
            active = {'announced': integer(item['test_count']),
                      'counts': dict.fromkeys(COUNTS, 0), 'names': set(),
                      'label': label or f'Suite {suites + 1}'}
        elif kind == 'test' and event in ('ok', 'failed', 'ignored'):
            if active is None or item.get('name') in active['names']:
                raise ValueError('test result outside a suite or duplicate result')
            active['names'].add(item['name'])
            active['counts'][{'ok': 'passed', 'failed': 'failed', 'ignored': 'skipped'}[event]] += 1
            if event == 'failed':
                output = item.get('stdout', '')
                if not isinstance(output, str):
                    raise ValueError('invalid failed-test output')
                output = re.sub(r'\x1b\[[0-9;]*[A-Za-z]', '', output)
                limit = min(4096, output_budget)
                excerpt = output[:limit]
                output_budget -= len(excerpt)
                failures.append(dict(suite=active['label'], name=item['name'],
                                     output=excerpt, truncated=len(output) > limit))
        elif kind == 'suite' and event in ('ok', 'failed'):
            if active is None:
                raise ValueError('suite result without start')
            for source, dest in [('passed', 'passed'), ('failed', 'failed'), ('ignored', 'skipped')]:
                if integer(item[source]) != active['counts'][dest]:
                    raise ValueError('libtest summary disagrees with test events')
            if integer(item.get('measured', 0)) or integer(item.get('filtered_out', 0)):
                raise ValueError('filtered/benchmark-only execution is not a full workspace run')
            close(False)
    close(True)
    if not suites:
        if command_passed:
            raise ValueError('successful workspace command produced no test events')
        return None
    total = sum(totals.values())
    if command_passed and (unfinished or totals['failed'] or totals['errors']):
        raise ValueError('successful command has failed/incomplete tests')
    return dict(totals, total=total, suites=suites,
                complete=bool(command_passed and not unfinished), command_passed=command_passed,
                failures=failures)


def validate_charts(charts):
    names = set()
    if not isinstance(charts, list) or len(charts) > len(CHARTS):
        raise ValueError('invalid chart collection')
    for chart in charts:
        if chart['name'] not in CHARTS or chart['name'] in names:
            raise ValueError('unexpected or duplicate chart')
        names.add(chart['name'])
        for field in ('title', 'unit', 'note'):
            if not isinstance(chart[field], str) or len(chart[field]) > 500:
                raise ValueError('invalid chart label')
        if not 1 <= len(chart['panels']) <= 4:
            raise ValueError('invalid panel count')
        for panel in chart['panels']:
            if not isinstance(panel['title'], str) or len(panel['title']) > 150:
                raise ValueError('invalid panel title')
            if not 1 <= len(panel['bars']) <= 40:
                raise ValueError('invalid bar count')
            for bar in panel['bars']:
                if not isinstance(bar['label'], str) or len(bar['label']) > 150:
                    raise ValueError('invalid bar label')
                v = bar['value']
                if v is not None and (type(v) not in (float, int) or not math.isfinite(v) or abs(v) > 1e20):
                    raise ValueError('invalid bar value')
    return charts


def collect(directory, kind):
    directory = Path(directory)
    lane = 'coverage' if kind == 'full' else 'benchmark'
    evidence_path = directory / lane / 'evidence.json'
    origin = None
    if os.environ.get("GITHUB_RUN_ID"):
        origin = dict(repository=os.environ["GITHUB_REPOSITORY"], run_id=int(os.environ["GITHUB_RUN_ID"]),
                      attempt=int(os.environ["GITHUB_RUN_ATTEMPT"]), commit=os.environ["GITHUB_SHA"])
    result = dict(format=FORMAT, kind=kind, origin=origin, source_id=None, tests=None, charts=[],
                  status='unavailable', note='No measurement evidence was produced.')
    if not evidence_path.exists():
        return result
    evidence = json.loads(evidence_path.read_text())
    if evidence['format'] != 1 or evidence['lane'] != lane:
        raise ValueError('unexpected evidence format/lane')
    source = evidence['source_id']
    if not isinstance(source, str) or len(source) != 64 or any(c not in '0123456789abcdef' for c in source):
        raise ValueError('invalid source fingerprint')
    result['source_id'] = source
    result['status'] = 'passed' if evidence['passed'] else 'failed'
    result['note'] = 'Evidence passed.' if evidence['passed'] else 'Lane failed or did not complete; inspect the linked CI run.'
    for step in evidence['steps']:
        name = step['name']
        if not name or any(not (c.isascii() and (c.isalnum() or c == '-')) for c in name):
            raise ValueError('unsafe log name')
        log = (directory / lane / 'logs' / f'{name}.log').read_bytes()
        if hashlib.sha256(log).hexdigest() != step['log_sha256']:
            raise ValueError('evidence log hash mismatch')
        if kind == 'full' and name == 'workspace-tests':
            command = step['command']
            if '--workspace' not in command or '--format=json' not in command or '--no-fail-fast' not in command:
                raise ValueError('test evidence is not the complete workspace JSON invocation')
            result['tests'] = test_counts(log.decode('utf-8'), step['passed'])
    # The Rust reporter only emits these after validating raw counters, source
    # identities, regression gates and benchmark samples. Never reuse stale charts.
    path = directory / 'charts' / 'data.json'
    if path.exists():
        data = json.loads(path.read_text())
        if data['source_id'] != source or not evidence['passed']:
            raise ValueError('stale/failed chart evidence')
        result['charts'] = validate_charts(data['charts'])
        if any((c['name'].startswith('coverage-')) != (kind == 'full') for c in result['charts']):
            raise ValueError('chart lane mismatch')
    return validate_publication(result)


def validate_publication(value):
    if set(value) != {'format', 'kind', 'origin', 'source_id', 'tests', 'charts', 'status', 'note'}:
        raise ValueError('unexpected publication fields')
    if value['format'] != FORMAT or value['kind'] not in ('full', 'benchmark'):
        raise ValueError('invalid publication format/kind')
    if value['status'] not in ('passed', 'failed', 'unavailable'):
        raise ValueError('invalid publication status')
    if not isinstance(value['note'], str) or len(value['note']) > 500:
        raise ValueError('invalid publication note')
    origin = value['origin']
    if origin is not None:
        import re
        if set(origin) != {'repository', 'run_id', 'attempt', 'commit'} or not re.fullmatch(r'[\w.-]+/[\w.-]+', origin['repository']):
            raise ValueError('invalid publication origin')
        if type(origin['run_id']) is not int or origin['run_id'] < 1 or type(origin['attempt']) is not int or origin['attempt'] < 1 or not re.fullmatch(r'[0-9a-f]{40}', origin['commit']):
            raise ValueError('invalid publication origin identity')
    source = value['source_id']
    if source is not None and (not isinstance(source, str) or len(source) != 64 or any(c not in '0123456789abcdef' for c in source)):
        raise ValueError('invalid source fingerprint')
    tests = value['tests']
    if tests is not None:
        required = {*COUNTS, 'total', 'suites', 'complete', 'command_passed'}
        if value['kind'] != 'full' or not required <= set(tests) or set(tests) - required - {'failures'}:
            raise ValueError('invalid test summary')
        if integer(tests['total']) != sum(integer(tests[k]) for k in COUNTS):
            raise ValueError('invalid aggregate test total')
        integer(tests['suites'])
        if type(tests['complete']) is not bool or type(tests['command_passed']) is not bool:
            raise ValueError('invalid test completion flag')
        if tests['complete'] and (not tests['command_passed'] or tests['errors'] or tests['failed']):
            raise ValueError('invalid successful test summary')
        # Older history has counts only. New reports retain every failed name,
        # with bounded diagnostic excerpts; full output remains in CI artifacts.
        if 'failures' in tests:
            failures = tests['failures']
            if not isinstance(failures, list) or len(failures) != tests['failed'] or len(failures) > 4096:
                raise ValueError('failed-test inventory disagrees with count or exceeds limit')
            for failure in failures:
                if not isinstance(failure, dict) or set(failure) != {'suite', 'name', 'output', 'truncated'}:
                    raise ValueError('invalid failed-test details')
                if any(not isinstance(failure[k], str) or len(failure[k]) > limit
                       for k, limit in [('suite', 1024), ('name', 1024), ('output', 4096)]):
                    raise ValueError('invalid failed-test text')
                if not failure['name'] or type(failure['truncated']) is not bool:
                    raise ValueError('invalid failed-test name or truncation flag')
            if sum(len(f['output']) for f in failures) > 256 * 1024:
                raise ValueError('failed-test output exceeds budget')
    validate_charts(value['charts'])
    if any(c['name'].startswith('coverage-') != (value['kind'] == 'full') for c in value['charts']):
        raise ValueError('chart lane mismatch')
    if (tests is not None or value['status'] == 'passed') and source is None:
        raise ValueError('measured results require a source fingerprint')
    if value['charts'] and (source is None or value['status'] != 'passed'):
        raise ValueError('charts require successful source-bound evidence')
    return value
