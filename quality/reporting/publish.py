"""Publish evidence to an isolated reports branch using atomic Git API updates."""
import base64
import hashlib
import io
import json
import os
from pathlib import Path
import tempfile
import time
from urllib.error import HTTPError
from urllib.parse import quote
from urllib.request import Request, urlopen
import zipfile

from .data import CHARTS, validate_publication
from .render import empty_history, merge, render

WORKFLOWS = {'.github/workflows/full-quality.yml': 'full',
             '.github/workflows/benchmarks.yml': 'benchmark',
             '.github/workflows/verification-coverage.yml': 'full',
             '.github/workflows/performance.yml': 'benchmark',
             '.github/workflows/verification-tests.yml': 'cargo',
             '.github/workflows/verification-models.yml': 'models',
             '.github/workflows/verification-fuzz.yml': 'fuzz'}
REPORT_BRANCH = 'reports'
MAX_ARTIFACT = 16 * 1024 * 1024
OWNED = {f'docs/reports/{name}.svg' for name in CHARTS} | {
    'README.md', 'docs/reports/history.json', 'docs/reports/test-history.svg',
    'docs/reports/cargo-history.svg', 'docs/reports/tla-history.svg',
    'docs/reports/failed-models.md', 'docs/reports/benchmarks-pending.svg', 'docs/reports/failed-tests.md'}


class GitHub:
    def __init__(self, repository, token):
        import re
        if not re.fullmatch(r'[\w.-]+/[\w.-]+', repository) or not token:
            raise ValueError('repository identity and GITHUB_TOKEN are required')
        self.base = f'https://api.github.com/repos/{repository}'
        self.token = token

    def request(self, path, body=None, method=None, raw=False):
        request = Request(self.base + path, method=method,
                          data=None if body is None else json.dumps(body).encode(),
                          headers={'Accept': 'application/vnd.github+json',
                                   'X-GitHub-Api-Version': '2022-11-28',
                                   'User-Agent': 'capnt-proto-readme', 'Content-Type': 'application/json'})
        # Artifact downloads redirect to blob storage. Do not forward the token.
        request.add_unredirected_header('Authorization', f'Bearer {self.token}')
        with urlopen(request, timeout=60) as response:
            data = response.read(MAX_ARTIFACT + 1 if raw else 16 * 1024 * 1024)
        if raw:
            if len(data) > MAX_ARTIFACT:
                raise ValueError('publication artifact too large')
            return data
        return json.loads(data) if data else None

    def content(self, path, ref):
        try:
            item = self.request(f'/contents/{path}?ref={quote(ref, safe="")}')
        except HTTPError as error:
            if error.code == 404:
                error.close()
                return None
            raise
        if item['type'] != 'file' or item['encoding'] != 'base64':
            raise ValueError('unexpected repository file representation')
        return base64.b64decode(item['content']), item['sha']


def trusted_run(run, repository, branch):
    return (run['status'] == 'completed' and run['path'] in WORKFLOWS
            and run['event'] in ('push', 'schedule', 'workflow_dispatch')
            and run['head_branch'] == branch and run['head_repository']['full_name'] == repository)


def artifact_data(api, run, kind):
    artifacts = api.request(f"/actions/runs/{run['id']}/artifacts?per_page=100")['artifacts']
    matches = [a for a in artifacts if a['name'] == 'readme-data' and not a['expired']]
    if not matches:
        # A failed/cancelled job may never have reached artifact upload. Record
        # the attempt without pretending that zero tests ran successfully.
        return dict(format=1, kind=kind, origin=None, source_id=None, tests=None, charts=[],
                    status='unavailable', note='This workflow attempt produced no public measurement artifact.')
    if len(matches) != 1 or matches[0]['size_in_bytes'] > MAX_ARTIFACT:
        raise ValueError('ambiguous or oversized publication artifact')
    content = api.request(f"/actions/artifacts/{matches[0]['id']}/zip", raw=True)
    with zipfile.ZipFile(io.BytesIO(content)) as archive:
        if archive.namelist() != ['publication.json'] or archive.getinfo('publication.json').file_size > MAX_ARTIFACT:
            raise ValueError('unexpected publication archive contents')
        data = validate_publication(json.loads(archive.read('publication.json')))
    expected = dict(repository=run['head_repository']['full_name'], run_id=run['id'], attempt=run['run_attempt'], commit=run['head_sha'])
    if data['origin'] != expected:
        return dict(format=1, kind=kind, origin=None, source_id=None, tests=None, charts=[], status='unavailable',
                    note='No measurement artifact matches this workflow attempt and commit.')
    if data['kind'] != kind:
        raise ValueError('publication workflow/lane mismatch')
    # An artifact from an earlier attempt must never make a cancelled rerun green.
    if run['conclusion'] != 'success' and data['status'] == 'passed':
        data['status'] = 'failed'
        data['charts'] = []
        data['note'] = 'Workflow failed after measurement; see the CI run.'
    return data


def blob_sha(data):
    return hashlib.sha1(f'blob {len(data)}\0'.encode() + data).hexdigest()


def publish(event_path):
    repository = os.environ['GITHUB_REPOSITORY']
    api = GitHub(repository, os.environ['GITHUB_TOKEN'])
    event = json.loads(Path(event_path).read_text())
    run_id = event.get('workflow_run', {}).get('id', event.get('inputs', {}).get('run_id'))
    if not str(run_id).isdigit():
        raise ValueError('a completed producer workflow run ID is required')
    branch = api.request('')['default_branch']
    run = api.request(f'/actions/runs/{int(run_id)}')
    if not trusted_run(run, repository, branch):
        raise ValueError('only trusted producer runs from this repository default branch can publish')
    kind = WORKFLOWS[run['path']]
    data = artifact_data(api, run, kind)
    record = dict(run_id=run['id'], attempt=run['run_attempt'], commit=run['head_sha'],
                  date=run['created_at'], url=run['html_url'], conclusion=run['conclusion'], data=data)
    if branch == REPORT_BRANCH:
        raise ValueError('report branch must not be the source default branch')
    for attempt in range(3):
        source_head = api.request(f'/git/ref/heads/{quote(branch, safe="")}')['object']['sha']
        comparison = api.request(f"/compare/{run['head_sha']}...{source_head}")
        if comparison['merge_base_commit']['sha'] != run['head_sha']:
            raise ValueError('measured commit is not an ancestor of the current default branch')
        try:
            head = api.request(f'/git/ref/heads/{REPORT_BRANCH}')['object']['sha']
        except HTTPError as error:
            if error.code != 404:
                raise
            error.close()
            head = None
        # Seed the new orphan branch once with the historical evidence from main.
        old = (api.content('docs/reports/history.json', head) if head else
               api.content('quality/reporting/history-seed.json', source_head))
        if head and old is None:
            raise ValueError('existing reports branch has no history; refusing to replace unrelated content')
        history = merge(json.loads(old[0]) if old else empty_history(), record)
        template = api.content('docs/reports.template.md', source_head)
        if template is None:
            raise ValueError('Report template is missing from the default branch')
        current_readme = api.content('README.md', head) if head else None
        try:
            existing = {item['path']: item['sha'] for item in (api.request(f'/contents/docs/reports?ref={head}') if head else []) if item['type'] == 'file'}
        except HTTPError as error:
            if error.code != 404:
                raise
            error.close()
            existing = {}
        if current_readme:
            existing['README.md'] = current_readme[1]
        with tempfile.TemporaryDirectory(prefix='capnt-readme-') as directory:
            root = Path(directory)
            files = render(template[0].decode(), history, root)
            generated = {p.relative_to(root).as_posix(): p.read_bytes() for p in files}
        if not generated.keys() <= OWNED:
            raise ValueError('renderer attempted to publish a non-report path')
        entries = []
        for path, content in generated.items():
            if existing.get(path) != blob_sha(content):
                blob = api.request('/git/blobs', {'content': base64.b64encode(content).decode(), 'encoding': 'base64'}, 'POST')
                entries.append(dict(path=path, mode='100644', type='blob', sha=blob['sha']))
        for path in existing.keys() & OWNED - generated.keys():
            entries.append(dict(path=path, mode='100644', type='blob', sha=None))
        if not entries:
            print('README reports are already current.')
            return
        tree_request = {'tree': entries}
        if head:
            tree_request['base_tree'] = api.request(f'/git/commits/{head}')['tree']['sha']
        tree = api.request('/git/trees', tree_request, 'POST')['sha']
        commit = api.request('/git/commits', {'message': f"docs: refresh Capntproto reports (run {run['id']})",
                                             'tree': tree, 'parents': [head] if head else []}, 'POST')['sha']
        try:
            if head:
                api.request(f'/git/refs/heads/{REPORT_BRANCH}', {'sha': commit, 'force': False}, 'PATCH')
            else:
                api.request('/git/refs', {'ref': f'refs/heads/{REPORT_BRANCH}', 'sha': commit}, 'POST')
            print(f'Published reports on {REPORT_BRANCH} at {commit}.')
            return
        except HTTPError as error:
            # A concurrent report publication/branch creation must be preserved.
            # Re-read history and retry; source refs are never mutated.
            if error.code not in (409, 422) or attempt == 2:
                raise
            error.close()
            time.sleep(1)
    raise RuntimeError('could not publish reports without overwriting concurrent work')
