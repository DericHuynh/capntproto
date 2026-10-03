"""Guard CI event routing and cancellation boundaries (requires PyYAML)."""
from pathlib import Path
import re
import unittest

import yaml


ROOT = Path(__file__).resolve().parents[2]


def concurrency_key(workflow, event, ref='refs/heads/main', pr=None):
    """Evaluate only the contexts used by our concurrency groups, not Actions code."""
    context = {
        'github.workflow': workflow['name'],
        'github.event_name': event,
        'github.ref': ref,
        'github.event.pull_request.number || github.ref': pr or ref,
    }
    return re.sub(r'\$\{\{\s*(.*?)\s*\}\}',
                  lambda match: str(context[match[1]]),
                  workflow['concurrency']['group']).casefold()


class WorkflowTriggerTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        paths = sorted((ROOT / '.github/workflows').glob('*.y*ml'))
        # BaseLoader keeps YAML's "on" key and boolean values as strings.
        cls.workflows = {p.stem: yaml.load(p.read_text(), Loader=yaml.BaseLoader)
                         for p in paths}

    def test_push_checks_are_only_for_main_and_have_pr_coverage(self):
        for name, workflow in self.workflows.items():
            if 'push' not in workflow['on']:
                continue
            with self.subTest(workflow=name):
                push = workflow['on']['push'] or {}
                # An exact branch allowlist also excludes tags. Branch ignores,
                # tag filters and wildcards could reintroduce overlapping runs.
                self.assertEqual(push.get('branches'), ['main'])
                self.assertLessEqual(set(push), {'branches', 'paths'})
                self.assertIn('pull_request', workflow['on'])

    def test_ci_has_one_automatic_entrypoint_and_explicit_dependencies(self):
        automatic = {name for name, w in self.workflows.items() if 'push' in w['on'] or 'pull_request' in w['on']}
        self.assertEqual(automatic, {'ci'})
        ci = self.workflows['ci']
        self.assertFalse(ci['on']['pull_request'])
        self.assertIn('workflow_dispatch', ci['on'])
        jobs = ci['jobs']
        for job, filename in [('workflows', 'ci-workflows'), ('documentation', 'ci-docs')]:
            self.assertEqual(jobs[job]['uses'], f'./.github/workflows/{filename}.yml')
            self.assertIn('workflow_call', self.workflows[filename]['on'])
        self.assertEqual(jobs['platforms']['needs'], 'workflows')
        self.assertEqual(set(jobs['report']['needs']), {'workflows', 'documentation', 'platforms'})
        self.assertEqual(jobs['report']['if'], 'always()')

    def test_pr_updates_cancel_only_the_same_check_for_the_same_pr(self):
        keys = set()
        for name in ('ci', 'ci-workflows', 'ci-docs'):
            workflow = self.workflows[name]
            self.assertEqual(workflow['concurrency']['cancel-in-progress'], 'true')
            # github.workflow in a called workflow is the caller's name. Check
            # these keys in that context so a reusable job cannot cancel CI.
            workflow = dict(workflow, name=self.workflows['ci']['name'])
            for pr in (1, 2):
                key = concurrency_key(workflow, 'pull_request', f'refs/pull/{pr}/merge', pr)
                self.assertNotIn(key, keys)
                keys.add(key)

    def test_pushes_do_not_cancel_manual_or_scheduled_link_checks(self):
        for name in ('ci', 'ci-workflows', 'ci-docs'):
            workflow = self.workflows[name]
            with self.subTest(workflow=name):
                keys = [concurrency_key(workflow, event)
                        for event in workflow['on'] if event not in ('pull_request', 'workflow_call')]
                self.assertEqual(len(keys), len(set(keys)))

    def test_expensive_verification_stays_out_of_push_and_pr_events(self):
        for name in ('verification-coverage', 'verification-extended'):
            self.assertEqual(set(self.workflows[name]['on']), {'schedule', 'workflow_dispatch'})
        self.assertEqual(set(self.workflows['performance']['on']), {'workflow_dispatch'})

    def test_resource_cleanup_and_publication_are_serialized_independently(self):
        groups = set()
        for name in ('maintenance-benchmarks', 'performance', 'reports'):
            workflow = self.workflows[name]
            with self.subTest(workflow=name):
                self.assertEqual(workflow['concurrency']['cancel-in-progress'], 'false')
                group = concurrency_key(workflow, 'workflow_dispatch', 'refs/heads/main')
                self.assertEqual(group, concurrency_key(workflow, 'schedule', 'refs/heads/other'))
                self.assertNotIn(group, groups)
                groups.add(group)

    def test_report_completion_does_not_start_tests_or_a_publication_loop(self):
        publishers = [w for w in self.workflows.values() if 'workflow_run' in w['on']]
        self.assertEqual(len(publishers), 1)
        events = publishers[0]['on']
        self.assertEqual(set(events), {'workflow_run', 'workflow_dispatch'})
        self.assertEqual(events['workflow_run']['types'], ['completed'])
        self.assertEqual(set(events['workflow_run']['workflows']), {
            self.workflows[name]['name'] for name in ('verification-coverage', 'performance')})


if __name__ == '__main__':
    unittest.main()
