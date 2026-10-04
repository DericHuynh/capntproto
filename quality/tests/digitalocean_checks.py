"""Fake-API checks invoked by the Cargo integration test, without cloud credentials."""
import importlib.util
import io
import json
import os
from pathlib import Path
import tempfile
import subprocess
from types import SimpleNamespace
import time
import urllib.error
from unittest.mock import patch

root = Path(__file__).resolve().parents[2]
spec = importlib.util.spec_from_file_location("digitalocean_bench", root / "scripts/digitalocean_bench.py")
cloud = importlib.util.module_from_spec(spec)
spec.loader.exec_module(cloud)
os.environ.update(GITHUB_REPOSITORY="test/capntproto", GITHUB_RUN_ID="42", GITHUB_RUN_ATTEMPT="1")


class API:
    def __init__(self):
        self.hosts = []
        self.keys = []
        self.calls = []
        self.price = 0.1
        self.creation_response_lost = False
        self.refuse_delete = False
        self.regions = ['nyc3']
        self.available = True

    def pages(self, path, key):
        if key == "sizes":
            return [{"slug": "c-4", "available": self.available, "regions": self.regions,
                     "price_hourly": self.price, "vcpus": 4}]
        if key == "droplets":
            return list(self.hosts)
        if key == "ssh_keys":
            return list(self.keys)
        raise AssertionError(path)

    def request(self, method, path, data=None):
        self.calls.append((method, path))
        if method == "POST" and path == "/account/keys":
            key = {"id": 9, "name": data["name"]}
            self.keys.append(key)
            return {"ssh_key": key}
        if method == "POST" and path == "/droplets":
            assert data["size"] == "c-4" and data["image"] == "ubuntu-24-04-x64"
            host = {"id": 7, "name": data["name"], "tags": data["tags"],
                    "status": "active", "networks": {"v4": [{"type": "public", "ip_address": "192.0.2.1"}]}}
            self.hosts.append(host)
            if self.creation_response_lost:
                raise RuntimeError("Lost creation response")
            return {"droplet": host}
        if method == "DELETE" and path.startswith("/droplets/"):
            if self.refuse_delete:
                raise RuntimeError("Deletion refused")
            self.hosts = [h for h in self.hosts if path != f'/droplets/{h["id"]}']
            return None
        if method == "GET" and path.startswith("/droplets/"):
            host = next((h for h in self.hosts if path == f'/droplets/{h["id"]}'), None)
            return {"droplet": host} if host else None
        if method == "DELETE" and path.startswith("/account/keys/"):
            self.keys = [k for k in self.keys if path != f'/account/keys/{k["id"]}']
            return None
        raise AssertionError((method, path))


def expect_error(operation, message):
    try:
        operation()
    except RuntimeError as error:
        assert message in str(error), str(error)
    else:
        raise AssertionError("Expected failure: " + message)


# Reject cost/CPU mistakes before any mutation.
with tempfile.TemporaryDirectory() as directory:
    base = Path(directory)
    for size, price, error in [("s-1vcpu-1gb", .1, "dedicated"), ("c-4", .51, "ceiling")]:
        api = API()
        api.price = price
        with patch.dict(os.environ, {"DO_SIZE": size, "DO_REGION": "nyc3"}):
            expect_error(lambda: cloud.run(api, base, base / "results", base / "state"), error)
        assert not api.calls

# A failed preflight replaces previous capacity and invalidates old samples.
with tempfile.TemporaryDirectory() as directory:
    base = Path(directory)
    api = API()
    api.available = False
    for name in ['trials.json', 'environment.json', 'instructions.jsonl', 'runner.log', 'capacity.json']:
        (base / name).write_text('stale')
    with patch.dict(os.environ, DO_SIZE='auto', DO_REGION='auto'):
        expect_error(lambda: cloud.run(api, base, base, base / 'state'), 'No available dedicated plan')
    assert not (base / 'trials.json').exists()
    assert json.loads((base / 'capacity.json').read_text())['status'] == 'unavailable'
    assert not api.calls

# Auto chooses live capacity without downgrading or overriding explicit input.
api = API()
api.regions = ['lon1', 'tor1']
with patch.dict(os.environ, DO_SIZE='auto', DO_REGION='auto'):
    size, region = cloud.preflight(api)
    assert size['slug'] == 'c-4' and region == 'tor1'
with patch.dict(os.environ, DO_SIZE='c-4', DO_REGION='nyc3'):
    expect_error(lambda: cloud.select_plan(api), 'c-4@tor1')
assert not api.calls
api.available = False
with patch.dict(os.environ, DO_SIZE='auto', DO_REGION='auto'):
    expect_error(lambda: cloud.select_plan(api), 'Available dedicated pairs: none')
for invalid_price in ['NaN', 'Infinity', '-0.1', '0', 'not-a-price']:
    api = API()
    api.price = invalid_price
    with patch.dict(os.environ, DO_SIZE='auto', DO_REGION='auto'):
        expect_error(lambda: cloud.select_plan(api), 'No available dedicated plan')
    assert not api.calls

class MixedCapacity(API):
    def pages(self, path, key):
        assert key == 'sizes'
        return [
            dict(slug='s-4vcpu-8gb', available=True, regions=['nyc3'], price_hourly=.01, vcpus=4),
            dict(slug='c-2', available=True, regions=['nyc3'], price_hourly=.05, vcpus=2),
            dict(slug='c-8', available=True, regions=['nyc3'], price_hourly=.2, vcpus=8),
            dict(slug='c-4', available=False, regions=['nyc3'], price_hourly=.1, vcpus=4),
            dict(slug='c-4-intel', available=True, regions=['lon1'], price_hourly=.15, vcpus=4),
        ]

with patch.dict(os.environ, DO_SIZE='auto', DO_REGION='auto'):
    size, region = cloud.select_plan(MixedCapacity())
    assert (size['slug'], region) == ('c-4-intel', 'lon1')

# Provider errors retain their explanation but never credentials or cloud-init.
token, private = 'PRIVATE_TOKEN_FIXTURE', 'PRIVATE_KEY_FIXTURE'
user_data = '#cloud-config\n' + json.dumps({'ssh_keys': {'ed25519_private': private}})
body = {'id': 'unprocessable_entity',
        'message': f'Size is not available in this region. {token} {private} {user_data}'}
error = urllib.error.HTTPError(cloud.API + '/droplets', 422, 'Unprocessable Entity', {},
                               io.BytesIO(json.dumps(body).encode()))
with patch.dict(os.environ, DIGITALOCEAN_ACCESS_TOKEN=token), \
        patch.object(cloud.urllib.request, 'urlopen', side_effect=error) as request:
    try:
        cloud.DigitalOcean().request('POST', '/droplets', {'user_data': user_data})
    except cloud.DigitalOceanError as failure:
        assert failure.capacity_rejection()
        assert failure.code == 'unprocessable_entity'
        assert 'Size is not available in this region' in str(failure)
        assert token not in str(failure) and private not in str(failure) and user_data not in str(failure)
    else:
        raise AssertionError('Expected provider error')
    assert request.call_count == 1  # The HTTP layer never repeats POST.

class RejectedCapacity(API):
    def __init__(self, message='Size is not available in this region.'):
        super().__init__()
        self.regions = ['nyc3', 'nyc1']
        self.message = message
        self.attempted = []

    def request(self, method, path, data=None):
        if method == 'POST' and path == '/droplets':
            self.attempted.append((data['size'], data['region']))
            if data['region'] == 'nyc3':
                raise cloud.DigitalOceanError(method, path, 422, 'unprocessable_entity', self.message)
        return super().request(method, path, data)

def provision(api, destination):
    size, region = cloud.select_plan(api)
    return cloud.create_host(api, {'name': 'fixture', 'tags': [cloud.owner_tag()],
                                   'image': 'ubuntu-24-04-x64'}, size, region, destination)

with tempfile.TemporaryDirectory() as directory:
    base = Path(directory)
    api = RejectedCapacity()
    with patch.dict(os.environ, DO_SIZE='auto', DO_REGION='auto'):
        host, size, region = provision(api, base)
    assert region == 'nyc1' and size['slug'] == 'c-4' and host['id'] == 7
    assert api.attempted == [('c-4', 'nyc3'), ('c-4', 'nyc1')]
    assert len(api.hosts) == 1
    assert [a['status'] for a in json.loads((base / 'provisioning.json').read_text())] == ['rejected', 'created']

    # A quota or configuration error is not a capacity fallback signal.
    for message in ('Droplet limit exceeded.', 'Invalid SSH key.', 'Payment required.'):
        api = RejectedCapacity(message)
        with patch.dict(os.environ, DO_SIZE='auto', DO_REGION='auto'):
            expect_error(lambda: provision(api, base), message)
        assert len(api.attempted) == 1 and not api.hosts

    # An explicitly requested region cannot silently move elsewhere.
    api = RejectedCapacity()
    with patch.dict(os.environ, DO_SIZE='c-4', DO_REGION='nyc3'):
        expect_error(lambda: provision(api, base), 'No available dedicated plan')
    assert api.attempted == [('c-4', 'nyc3')]

    api = RejectedCapacity()
    with patch.dict(os.environ, DO_SIZE='auto', DO_REGION='auto'), patch.object(cloud, 'MAX_CREATE_ATTEMPTS', 1):
        expect_error(lambda: provision(api, base), 'fallback limit reached')
    assert len(api.attempted) == 1

    # Even a capacity rejection must not duplicate an unexpectedly created host.
    api = RejectedCapacity()
    api.hosts = [{'id': 7, 'name': 'fixture', 'tags': [cloud.owner_tag()]}]
    with patch.dict(os.environ, DO_SIZE='auto', DO_REGION='auto'):
        expect_error(lambda: provision(api, base), 'refusing another POST')
    assert len(api.attempted) == 1

# Creation can succeed even if the response is lost. Cleanup finds the unique name.
def fake_command(args, **kwargs):
    assert args[0] == "ssh-keygen"
    path = Path(args[-1])
    path.write_text("PRIVATE FIXTURE")
    path.with_suffix(".pub").write_text("ssh-ed25519 PUBLIC FIXTURE")

with tempfile.TemporaryDirectory() as directory:
    base = Path(directory)
    api = API()
    api.creation_response_lost = True
    with patch.object(cloud, "command", fake_command), patch.dict(os.environ, {"DO_SIZE": "c-4", "DO_REGION": "nyc3"}):
        expect_error(lambda: cloud.run(api, base, base / "results", base / "state"), "Lost creation response")
    assert not api.hosts and not api.keys
    assert (base / "state/receipt.json").exists()
    assert not (base / "state/client").exists()
    cloud.cleanup(api, json.loads((base / "state/receipt.json").read_text())["name"])

# A failed remote driver streams progress, preserves its status, retrieves
# partial evidence and destroys the host even when one diagnostic is missing.
with tempfile.TemporaryDirectory() as directory:
    base = Path(directory)
    bundle = base / 'bundle'
    bundle.mkdir()
    (bundle / 'driver').write_text('fixture')
    api = API()
    downloads = []
    def remote_command(args, **kwargs):
        if args[0] == 'ssh-keygen':
            return fake_command(args, **kwargs)
        if args[0] == 'ssh' and 'pipefail' in args[-1]:
            assert 'tee /root/results/runner.log' in args[-1]
            raise subprocess.CalledProcessError(124, args)
        if args[0] == 'scp' and ':/root/results/' in args[-2]:
            name = args[-2].rsplit('/', 1)[1]
            downloads.append(name)
            if name == 'instructions.jsonl':
                raise subprocess.CalledProcessError(1, args)
            Path(args[-1]).write_text('partial evidence')
    with patch.object(cloud, 'command', remote_command), patch.object(cloud.subprocess, 'run', return_value=SimpleNamespace(returncode=0)):
        with patch.dict(os.environ, DO_SIZE='auto', DO_REGION='auto'):
            try:
                cloud.run(api, bundle, base / 'results', base / 'state')
            except subprocess.CalledProcessError as error:
                assert error.returncode == 124
            else:
                raise AssertionError('Expected measurement timeout')
    assert downloads == ['runner.log', 'trials.json', 'environment.json', 'instructions.jsonl']
    assert (base / 'results/runner.log').read_text() == 'partial evidence'
    assert not api.hosts and not api.keys

# TTL cleanup preserves active hosts and unrelated resources, and surfaces failures.
api = API()
prefix = cloud.owner_tag() + "-"
expired = prefix + str(int(time.time()) - cloud.TTL - 100) + "-1"
active = prefix + str(int(time.time())) + "-2"
api.hosts = [
    {"id": 1, "name": expired, "tags": [cloud.owner_tag()], "created_at": "2000-01-01T00:00:00Z"},
    {"id": 2, "name": active, "tags": [cloud.owner_tag()], "created_at": "2100-01-01T00:00:00Z"},
    {"id": 3, "name": "unrelated", "tags": ["production"], "created_at": "2000-01-01T00:00:00Z"},
]
api.keys = [{"id": 1, "name": expired}, {"id": 2, "name": active}]
cloud.janitor(api)
assert [h["id"] for h in api.hosts] == [2, 3]
assert [k["id"] for k in api.keys] == [2]
api.refuse_delete = True
expect_error(lambda: cloud.cleanup(api, active), "Deletion refused")
assert len(api.hosts) == 2
print("DigitalOcean cost bounds, lost-response recovery, TTL scope and cleanup failure checks passed")
