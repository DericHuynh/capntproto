import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
from urllib.error import HTTPError
import zipfile

from reporting.data import collect, test_counts, validate_publication
from reporting.publish import GitHub, OWNED, artifact_data, blob_sha, publish, trusted_run
from reporting.render import empty_history, failure_report, merge, render, validate_history


def events(*items):
    return '\n'.join(json.dumps(i) for i in items)


def suite(passed=2, failed=1, ignored=1):
    rows = [dict(type='suite', event='started', test_count=passed + failed + ignored)]
    for event, count in [('ok', passed), ('failed', failed), ('ignored', ignored)]:
        rows += [dict(type='test', event=event, name=f'{event}_{i}', stdout='test result: ok. 99 passed;\n{"type":"suite"}') for i in range(count)]
    rows.append(dict(type='suite', event='failed' if failed else 'ok', passed=passed,
                     failed=failed, ignored=ignored, measured=0, filtered_out=0))
    return events(*rows)


def publication(kind='full'):
    return dict(format=1, kind=kind, origin=None, source_id='a' * 64,
                status='passed', note='Synthetic unit-test fixture, not a measurement.',
                tests=test_counts(suite(3, 0, 1), True) if kind == 'full' else None, charts=[])


def record(run_id=1, kind='full', date='2026-09-28T10:00:00Z', attempt=1):
    return dict(run_id=run_id, attempt=attempt, date=date, commit='b' * 40,
                url=f'https://github.com/example/capnt-proto/actions/runs/{run_id}',
                conclusion='success', data=publication(kind))


class Counts(unittest.TestCase):
    def test_counts_failures_ignored_doctests_without_nested_output(self):
        counts = test_counts('cargo output\n' + suite() + '\n' + suite(5, 0, 0), False)
        self.assertEqual([counts[k] for k in ('total', 'passed', 'failed', 'skipped', 'errors', 'suites')], [9, 7, 1, 1, 0, 2])
        self.assertFalse(counts['complete'])

    def test_all_failures_keep_suite_identity_and_escape_diagnostics(self):
        output = '    Running tests/a.rs (target/a)\n' + suite(0, 1, 0)
        output += '\n   Doc-tests example\n' + suite(0, 1, 0)
        counts = test_counts(output, False)
        self.assertEqual(len(counts['failures']), 2)
        self.assertNotEqual(counts['failures'][0]['suite'], counts['failures'][1]['suite'])
        entry = record()
        entry['data']['tests'] = counts
        counts['failures'][0]['output'] = '<script>alert(1)</script> [click](javascript:x)'
        report = failure_report(entry)
        self.assertEqual(report.count('<details open>'), 2)
        self.assertIn('&lt;script&gt;', report)
        self.assertNotIn('<script>', report)
        self.assertIn(entry['url'], report)
        self.assertIn('**2 failed**', report)

    def test_failure_diagnostics_are_bounded_and_validated(self):
        counts = test_counts(suite(0, 80, 0).replace('test result: ok. 99 passed;', 'x' * 5000), False)
        self.assertEqual(len(counts['failures']), 80)
        self.assertTrue(all(f['truncated'] for f in counts['failures']))
        self.assertLessEqual(sum(len(f['output']) for f in counts['failures']), 256 * 1024)
        entry = publication()
        entry['tests'] = counts
        validate_publication(entry)
        counts['failures'].pop()
        with self.assertRaises(ValueError):
            validate_publication(entry)
        del counts['failures']  # Existing count-only history remains readable.
        validate_publication(entry)

    def test_abort_and_build_failures_are_distinct(self):
        self.assertIsNone(test_counts('error: could not compile crate', False))
        partial = events(dict(type='suite', event='started', test_count=4),
                         dict(type='test', event='ok', name='finished'))
        counts = test_counts(partial, False)
        self.assertEqual((counts['total'], counts['passed'], counts['errors']), (4, 1, 3))
        counts = test_counts(partial + '\n' + suite(2, 0, 0), False)
        self.assertEqual((counts['total'], counts['passed'], counts['errors']), (6, 3, 3))
        with self.assertRaises(ValueError):
            test_counts(partial, True)

    def test_duplicate_filtered_and_inconsistent_counts_rejected(self):
        source = suite()
        for invalid in [source.replace('"passed": 2', '"passed": 3'),
                        source.replace('"filtered_out": 0', '"filtered_out": 8'),
                        source.replace('ok_1', 'ok_0')]:
            with self.assertRaises(ValueError):
                test_counts(invalid, False)

    def test_collector_verifies_source_and_log_integrity(self):
        with tempfile.TemporaryDirectory() as directory, patch.dict(os.environ, {}, clear=True):
            root = Path(directory)
            self.assertEqual(collect(root, 'full')['status'], 'unavailable')
            lane = root / 'coverage'
            (lane / 'logs').mkdir(parents=True)
            log = suite(3, 0, 1).encode()
            (lane / 'logs/workspace-tests.log').write_bytes(log)
            evidence = dict(format=1, lane='coverage', source_id='a' * 64, passed=True,
                            steps=[dict(name='workspace-tests', command=['cargo', 'test', '--workspace', '--no-fail-fast', '--', '-Z', 'unstable-options', '--format=json'],
                                        passed=True, log_sha256=hashlib.sha256(log).hexdigest())])
            (lane / 'evidence.json').write_text(json.dumps(evidence))
            self.assertEqual(collect(root, 'full')['tests']['total'], 4)
            (lane / 'logs/workspace-tests.log').write_text(suite())
            with self.assertRaisesRegex(ValueError, 'hash mismatch'):
                collect(root, 'full')

    def test_malicious_chart_names_and_nonfinite_values_rejected(self):
        value = publication('benchmark')
        value['charts'] = [dict(name='../README', title='x', unit='x', note='', panels=[])]
        with self.assertRaises(ValueError):
            validate_publication(value)
        value['charts'][0].update(name='latency-p50', panels=[dict(title='1', bars=[dict(label='x', value=float('nan'))])])
        with self.assertRaises(ValueError):
            validate_publication(value)


class History(unittest.TestCase):
    def test_ordering_idempotence_reruns_and_failed_benchmarks(self):
        history = merge(empty_history(), record(2, date='2026-09-29T10:00:00Z'))
        history = merge(history, record())
        self.assertEqual([r['run_id'] for r in history['full']], [1, 2])
        self.assertEqual(merge(history, record()), history)
        rerun = record(attempt=2)
        rerun['conclusion'] = 'failure'
        rerun['data']['status'] = 'failed'
        rerun['data']['tests'] = None
        history = merge(history, rerun)
        self.assertEqual(history['full'][0]['attempt'], 2)
        self.assertIsNone(history['full'][0]['data']['tests'])
        history = merge(history, record(3, 'benchmark'))
        failure = record(4, 'benchmark', '2026-09-29T10:00:00Z')
        failure['data'].update(status='failed', charts=[])
        history = merge(history, failure)
        self.assertEqual(history['benchmark']['run_id'], 4)
        self.assertEqual(history['benchmark']['data']['charts'], [])

    def test_invalid_history_cannot_inject_links_or_counts(self):
        for change in [dict(url='javascript:alert(1)'), dict(commit='../README'), dict(date='today')]:
            entry = record()
            entry.update(change)
            with self.assertRaises(ValueError):
                merge(empty_history(), entry)
        entry = record()
        entry['data']['tests']['total'] = 500
        with self.assertRaises(ValueError):
            merge(empty_history(), entry)

    def test_history_bounded_and_historical_coverage_removed(self):
        history = empty_history()
        for i in range(370):
            history = merge(history, record(i + 1))
        self.assertEqual(len(history['full']), 365)
        validate_history(history)
        self.assertNotIn('failures', history['full'][0]['data']['tests'])
        self.assertIn('failures', history['full'][-1]['data']['tests'])


class Publishing(unittest.TestCase):
    def test_archive_paths_and_rerun_identity_are_checked(self):
        run = dict(id=1, run_attempt=2, head_sha='b' * 40, conclusion='success',
                   head_repository=dict(full_name='example/capnt-proto'))
        data = publication()
        data['origin'] = dict(repository='example/capnt-proto', run_id=1, attempt=1, commit='b' * 40)
        class API:
            def __init__(self, name):
                buffer = io.BytesIO()
                with zipfile.ZipFile(buffer, 'w') as archive:
                    archive.writestr(name, json.dumps(data))
                self.archive = buffer.getvalue()
            def request(self, path, raw=False):
                if raw:
                    return self.archive
                return dict(artifacts=[dict(id=9, name='readme-data', expired=False, size_in_bytes=len(self.archive))])
        with self.assertRaises(ValueError):
            artifact_data(API('../publication.json'), run, 'full')
        self.assertEqual(artifact_data(API('publication.json'), run, 'full')['status'], 'unavailable')
        data['origin']['attempt'] = 2
        self.assertEqual(artifact_data(API('publication.json'), run, 'full')['tests']['total'], 4)

    def test_atomic_publish_retries_and_changes_only_owned_paths(self):
        calls = []
        class API:
            def __init__(self, *args):
                self.attempt = 0
            def content(self, path, ref):
                if path == 'docs/README.template.md':
                    return (f'# Template at {ref}\n{{{{REPORTS}}}}'.encode(), 'f' * 40)
                return None
            def request(self, path, body=None, method=None):
                calls.append((path, body, method))
                if path == '':
                    return dict(default_branch='main')
                if path == '/actions/runs/1':
                    return dict(id=1, run_attempt=1, path='.github/workflows/verification-coverage.yml', status='completed',
                                event='schedule', head_branch='main', head_repository=dict(full_name='example/capnt-proto'),
                                head_sha='b' * 40, created_at='2026-09-28T10:00:00Z', conclusion='success',
                                html_url='https://github.com/example/capnt-proto/actions/runs/1')
                if path == '/git/ref/heads/main':
                    return dict(object=dict(sha=('c' if self.attempt == 0 else 'd') * 40))
                if path.startswith('/compare/'):
                    return dict(merge_base_commit=dict(sha='b' * 40))
                if path.startswith('/contents/docs/reports'):
                    return [dict(path='docs/reports/latency-p50.svg', sha='e' * 40, type='file'),
                            dict(path='docs/reports/keep.txt', sha='f' * 40, type='file')]
                if path.startswith('/git/commits/'):
                    return dict(tree=dict(sha='e' * 40))
                if path == '/git/refs/heads/main':
                    self.attempt += 1
                    if self.attempt == 1:
                        raise HTTPError('https://api.github.com/', 409, 'concurrent update', None, None)
                    return {}
                if path in ('/git/trees', '/git/blobs', '/git/commits'):
                    return dict(sha='a' * 40)
                raise AssertionError(path)
        def renderer(template, history, root):
            root.joinpath('README.md').write_text(template)
            return [root / 'README.md']
        with tempfile.TemporaryDirectory() as directory:
            event = Path(directory) / 'event.json'
            event.write_text('{"workflow_run":{"id":1}}')
            with patch.dict(os.environ, GITHUB_REPOSITORY='example/capnt-proto', GITHUB_TOKEN='test-token'), \
                    patch('reporting.publish.GitHub', API), patch('reporting.publish.artifact_data', return_value=publication()), \
                    patch('reporting.publish.render', renderer), patch('reporting.publish.time.sleep'):
                publish(event)
        trees = [body for path, body, _ in calls if path == '/git/trees']
        self.assertEqual(len(trees), 2)
        self.assertTrue(all(entry['path'] in OWNED for tree in trees for entry in tree['tree']))
        self.assertTrue(any(entry['path'] == 'docs/reports/latency-p50.svg' and entry['sha'] is None for entry in trees[0]['tree']))
        commits = [body for path, body, _ in calls if path == '/git/commits']
        self.assertEqual([c['parents'] for c in commits], [['c' * 40], ['d' * 40]])
        self.assertTrue(all(body['force'] is False for path, body, _ in calls if path == '/git/refs/heads/main'))

    def test_untrusted_origins_and_workflows_cannot_publish(self):
        run = dict(status='completed', path='.github/workflows/verification-coverage.yml', event='schedule',
                   head_branch='main', head_repository=dict(full_name='example/capnt-proto'))
        self.assertTrue(trusted_run(run, 'example/capnt-proto', 'main'))
        for change in [dict(event='pull_request'), dict(head_branch='topic'), dict(path='.github/workflows/unknown.yml'),
                       dict(head_repository=dict(full_name='fork/capnt-proto'))]:
            self.assertFalse(trusted_run(dict(run, **change), 'example/capnt-proto', 'main'))

    def test_missing_artifact_is_unknown_not_green(self):
        class API:
            def request(self, path):
                return {'artifacts': []}
        value = artifact_data(API(), {'id': 1}, 'full')
        self.assertEqual(value['status'], 'unavailable')
        self.assertIsNone(value['tests'])
        self.assertEqual(blob_sha(b'hello\n'), 'ce013625030ba8dba906f756967f9e9ca394464a')

    def test_authorization_is_not_forwarded_to_storage_redirects(self):
        with patch('reporting.publish.urlopen') as opened:
            opened.return_value.__enter__.return_value.read.return_value = b'{}'
            GitHub('example/capnt-proto', 'test-token').request('/actions/runs/1')
            req = opened.call_args.args[0]
            self.assertNotIn('Authorization', req.headers)
            self.assertEqual(req.unredirected_hdrs['Authorization'], 'Bearer test-token')


class Rendering(unittest.TestCase):
    def test_empty_and_measured_reports_have_labels_and_never_keep_stale_bars(self):
        if importlib.util.find_spec('matplotlib') is None:
            self.skipTest('chart rendering requires quality/reporting-requirements.txt; CI installs it')
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            template = '# Synthetic renderer fixture — not measured results\n\n{{REPORTS}}'
            files = render(template, empty_history(), output)
            self.assertTrue({p.relative_to(output).as_posix() for p in files} <= OWNED)
            self.assertIn('Awaiting', (output / 'README.md').read_text())
            self.assertNotIn('0 passed', (output / 'README.md').read_text())
            history = merge(empty_history(), record())
            benchmark = record(3, 'benchmark')
            benchmark['data']['charts'] = [dict(name='latency-p50', title='Synthetic renderer data — Median latency',
                unit='Microseconds — lower is better', note='Test fixture, not a performance claim.', coverage=False,
                panels=[dict(title=f'{size} byte payload', bars=[dict(label=label, value=10.0 * (i + 1))
                        for i, label in enumerate(["Capntproto / Native", "Cap'n Proto C++", 'gRPC (tonic)', 'WebSockets'])])
                        for size in (0, 64, 1024, 65536)])]
            history = merge(history, benchmark)
            render(template, history, output)
            svg = (output / 'docs/reports/latency-p50.svg').read_text()
            for label in ['gRPC (tonic)', 'WebSockets', '10.00', '65536 byte payload', 'Microseconds']:
                self.assertIn(label, svg)
            first = (output / 'docs/reports/test-history.svg').read_bytes()
            render(template, history, output)
            self.assertEqual(first, (output / 'docs/reports/test-history.svg').read_bytes())
            if os.environ.get('CAPNT_REPORT_PREVIEW'):
                import shutil
                shutil.copytree(output, os.environ['CAPNT_REPORT_PREVIEW'], dirs_exist_ok=True)
            render(template, empty_history(), output)
            self.assertFalse((output / 'docs/reports/latency-p50.svg').exists())
            self.assertTrue((output / 'docs/reports/benchmarks-pending.svg').exists())


if __name__ == '__main__':
    unittest.main()
