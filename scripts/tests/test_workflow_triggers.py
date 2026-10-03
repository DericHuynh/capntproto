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

    def test_pr_checks_remain_available_after_removing_branch_pushes(self):
        for name in ('quality', 'workflow-checks', 'links'):
            with self.subTest(workflow=name):
                events = self.workflows[name]['on']
                pr = events['pull_request'] or {}
                self.assertLessEqual(set(pr), {'paths'})
                self.assertEqual(pr.get('paths'), events['push'].get('paths'))
                self.assertIn('workflow_dispatch', events)
        # This is the unconditional required check; path filtering would leave
        # some PRs waiting for a check that was never scheduled.
        self.assertFalse(self.workflows['quality']['on']['pull_request'])

    def test_pr_updates_cancel_only_the_same_check_for_the_same_pr(self):
        keys = set()
        for name, workflow in self.workflows.items():
            if 'pull_request' not in workflow['on']:
                continue
            with self.subTest(workflow=name):
                self.assertEqual(workflow['concurrency']['cancel-in-progress'], 'true')
                first = concurrency_key(workflow, 'pull_request', 'refs/pull/1/merge', 1)
                other = concurrency_key(workflow, 'pull_request', 'refs/pull/2/merge', 2)
                self.assertNotEqual(first, other)
                self.assertNotIn(first, keys)
                self.assertNotIn(other, keys)
                keys.update((first, other))

    def test_pushes_do_not_cancel_manual_or_scheduled_link_checks(self):
        for name in ('quality', 'workflow-checks', 'links'):
            workflow = self.workflows[name]
            with self.subTest(workflow=name):
                keys = [concurrency_key(workflow, event)
                        for event in workflow['on'] if event != 'pull_request']
                self.assertEqual(len(keys), len(set(keys)))

    def test_expensive_verification_stays_out_of_push_and_pr_events(self):
        for name in ('full-quality', 'extended-quality'):
            self.assertEqual(set(self.workflows[name]['on']), {'schedule', 'workflow_dispatch'})
        self.assertEqual(set(self.workflows['benchmarks']['on']), {'workflow_dispatch'})

    def test_resource_cleanup_and_publication_are_serialized_independently(self):
        groups = set()
        for name in ('benchmark-cleanup', 'benchmarks', 'readme'):
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
            self.workflows[name]['name'] for name in ('full-quality', 'benchmarks')})


if __name__ == '__main__':
    unittest.main()
