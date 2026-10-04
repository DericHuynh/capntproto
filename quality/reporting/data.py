"""Read only validated CI evidence; preserve failures and unknown measurements."""
import hashlib
import json
import math
import os
from pathlib import Path
import re
import xml.etree.ElementTree as ET

FORMAT = 1
CHARTS = {
    'latency-p50', 'latency-p95', 'latency-p99', 'request-rate',
    'latency-difference', 'request-rate-difference', 'instruction-counts',
    'fuzz-executions', 'fuzz-coverage', 'fuzz-findings', 'tla-outcomes', 'tla-states',
    'coverage-lines', 'coverage-regions', 'coverage-functions', 'coverage-branches',
}
COUNTS = ('passed', 'failed', 'errors', 'skipped')


def integer(value):
    if type(value) is not int or not 0 <= value <= 10_000_000:
        raise ValueError('invalid test counter')
    return value


def test_counts(log, command_passed, *, allow_filtered=False):
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
            if integer(item.get('measured', 0)) or (integer(item.get('filtered_out', 0)) and not allow_filtered):
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


def junit_counts(xml, command_passed):
    """Read nextest's stable JUnit, including ignored tests and abort diagnostics."""
    if len(xml) > 64 * 1024 * 1024:
        raise ValueError('oversized JUnit report')
    # Nextest writes UTF-8. Reject alternate encodings before checking DTDs, so
    # UTF-16 cannot hide entity declarations from the validation below.
    text = xml.decode('utf-8')
    if '\0' in text or '<!DOCTYPE' in text or '<!ENTITY' in text:
        raise ValueError('unsafe JUnit report')
    try:
        root = ET.fromstring(text)
    except ET.ParseError as error:
        raise ValueError('invalid JUnit report') from error
    if root.tag != 'testsuites':
        raise ValueError('expected nextest testsuites')
    totals = dict.fromkeys(COUNTS, 0)
    identities, failures = set(), []
    budget = 256 * 1024
    suites = root.findall('testsuite')

    def validate(node, counts):
        for attr, expected in [('tests', sum(counts.values())), ('failures', counts['failed']),
                               ('errors', counts['errors']), ('skipped', counts['skipped'])]:
            value = node.get(attr)
            if value is None or not value.isascii() or not value.isdigit() or integer(int(value)) != expected:
                raise ValueError('JUnit counters disagree with test cases')

    for suite in suites:
        counts = dict.fromkeys(COUNTS, 0)
        for case in suite.findall('testcase'):
            name, label = case.get('name'), case.get('classname', suite.get('name'))
            if not name or not label or (label, name) in identities:
                raise ValueError('missing or duplicate JUnit test identity')
            identities.add((label, name))
            outcomes = [c for c in case if c.tag in ('failure', 'error', 'skipped')]
            if len(outcomes) > 1:
                raise ValueError('conflicting JUnit outcomes')
            outcome = outcomes[0].tag if outcomes else 'passed'
            key = {'failure': 'failed', 'error': 'errors', 'skipped': 'skipped'}.get(outcome, 'passed')
            counts[key] += 1
            if key in ('failed', 'errors'):
                output = '\n'.join(''.join(c.itertext()) for c in case if c.tag in ('failure', 'error', 'system-out', 'system-err'))
                output = re.sub(r'\x1b\[[0-9;]*[A-Za-z]', '', output)
                limit = min(4096, budget)
                failures.append(dict(suite=label, name=name, output=output[:limit], truncated=len(output) > limit))
                budget -= len(output[:limit])
        validate(suite, counts)
        for key in COUNTS:
            totals[key] += counts[key]
    validate(root, totals)
    if command_passed and (not suites or totals['failed'] or totals['errors']):
        raise ValueError('successful nextest command has missing/failed tests')
    return dict(totals, total=sum(totals.values()), suites=len(suites),
                complete=command_passed, command_passed=command_passed, failures=failures)


def merge_test_counts(native, docs):
    # A build failure has unknown totals; never present the other phase as a full run.
    if native is None or docs is None:
        known = native or docs
        if known is None:
            return None
        return dict(known, complete=False, command_passed=False)
    budget, failures = 256 * 1024, []
    for failure in native['failures'] + docs['failures']:
        excerpt = failure['output'][:budget]
        failures.append(dict(failure, output=excerpt, truncated=failure['truncated'] or len(excerpt) < len(failure['output'])))
        budget -= len(excerpt)
    return dict({key: native[key] + docs[key] for key in (*COUNTS, 'total', 'suites')},
                complete=native['complete'] and docs['complete'],
                command_passed=native['command_passed'] and docs['command_passed'],
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
                if chart['name'].startswith(('fuzz-', 'tla-')) and v is not None and (type(v) is not int or v < 0):
                    raise ValueError('verification counters must be nonnegative integers')
    return charts


def collect(directory, kind):
    directory = Path(directory)
    lane = {'full': 'coverage', 'cargo': 'coverage', 'models': 'models', 'fuzz': 'fuzz', 'benchmark': 'benchmark'}[kind]
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
    nextest = False
    docs = None
    for step in evidence['steps']:
        name = step['name']
        if not name or any(not (c.isascii() and (c.isalnum() or c == '-')) for c in name):
            raise ValueError('unsafe log name')
        log = (directory / lane / 'logs' / f'{name}.log').read_bytes()
        if hashlib.sha256(log).hexdigest() != step['log_sha256']:
            raise ValueError('evidence log hash mismatch')
        if (kind in ('full', 'cargo') and name == 'workspace-tests') or (kind == 'models' and name == 'model-tests'):
            command = step['command']
            if '--workspace' not in command or '--no-fail-fast' not in command:
                raise ValueError('test evidence is not the complete workspace invocation')
            if kind == 'models' and 'tlc' not in command:
                raise ValueError('model partition must select TLC tests')
            if 'nextest' in command:
                nextest = True
                if kind == 'cargo' and '-E' not in command:
                    raise ValueError('Cargo partition must exclude dedicated checks')
                report = evidence.get('data', {}).get('test_reports', {}).get(name)
                if report is None:
                    if step['passed']:
                        raise ValueError('successful nextest run is missing JUnit')
                    result['tests'] = None
                else:
                    if report['file'] != f'{name}.xml':
                        raise ValueError('unexpected JUnit filename')
                    xml = (directory / lane / 'logs' / report['file']).read_bytes()
                    if hashlib.sha256(xml).hexdigest() != report['sha256']:
                        raise ValueError('JUnit hash mismatch')
                    result['tests'] = junit_counts(xml, step['passed'])
            else:
                # Historical evidence remains readable after the runner migration.
                if '--format=json' not in command:
                    raise ValueError('test evidence is not a JSON invocation')
                if kind == 'cargo' and '--skip' not in command:
                    raise ValueError('Cargo partition must exclude dedicated checks')
                result['tests'] = test_counts(log.decode('utf-8'), step['passed'], allow_filtered=kind != 'full')
        elif name == 'workspace-doctests' and kind in ('full', 'cargo'):
            if not all(arg in step['command'] for arg in ('--workspace', '--doc', '--format=json')):
                raise ValueError('incomplete doctest invocation')
            docs = test_counts(log.decode('utf-8'), step['passed'])
    if nextest and kind in ('full', 'cargo'):
        if evidence['passed'] and docs is None:
            raise ValueError('successful coverage lane is missing doctests')
        result['tests'] = merge_test_counts(result['tests'], docs)
    # The Rust reporter only emits these after validating raw counters, source
    # identities, regression gates and benchmark samples. Never reuse stale charts.
    path = directory / 'charts' / 'data.json'
    if path.exists():
        data = json.loads(path.read_text())
        if data['source_id'] != source or not evidence['passed']:
            raise ValueError('stale/failed chart evidence')
        result['charts'] = validate_charts(data['charts'])
        if any(not chart_kind(c['name'], kind) for c in result['charts']):
            raise ValueError('chart lane mismatch')
    if kind in ('models', 'fuzz'):
        from . import verification
        result['charts'] = (verification.models(evidence['data'], directory / lane)
                            if kind == 'models' else verification.fuzz(evidence['data']))
    return validate_publication(result)


def chart_kind(name, kind):
    if kind in ('full', 'cargo'):
        return name.startswith('coverage-')
    if kind == 'models':
        return name.startswith('tla-')
    if kind == 'fuzz':
        return name.startswith('fuzz-')
    return not name.startswith(('coverage-', 'tla-', 'fuzz-'))


def validate_publication(value):
    if set(value) != {'format', 'kind', 'origin', 'source_id', 'tests', 'charts', 'status', 'note'}:
        raise ValueError('unexpected publication fields')
    if value['format'] != FORMAT or value['kind'] not in ('full', 'cargo', 'models', 'fuzz', 'benchmark'):
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
        if value['kind'] not in ('full', 'cargo', 'models') or not required <= set(tests) or set(tests) - required - {'failures'}:
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
            if not isinstance(failures, list) or not tests['failed'] <= len(failures) <= tests['failed'] + tests['errors'] or len(failures) > 4096:
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
    if any(not chart_kind(c['name'], value['kind']) for c in value['charts']):
        raise ValueError('chart lane mismatch')
    if (tests is not None or value['status'] == 'passed') and source is None:
        raise ValueError('measured results require a source fingerprint')
    if value['charts'] and (source is None or (value['status'] != 'passed' and value['kind'] not in ('models', 'fuzz'))):
        raise ValueError('charts require successful source-bound evidence')
    return value
