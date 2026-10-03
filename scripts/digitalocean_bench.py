#!/usr/bin/env python3
"""Ephemeral benchmark host lifecycle. Workloads themselves are Rust/Cargo binaries.

The only credential is DIGITALOCEAN_ACCESS_TOKEN, provided to this step by Actions.
No credential, private SSH key or cloud-init body is written to report artifacts.
"""
import argparse
import datetime
import hashlib
import ipaddress
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import sys
import tarfile
import time
import urllib.error
import urllib.parse
import urllib.request

API = "https://api.digitalocean.com/v2"
TTL = 2 * 60 * 60


class DigitalOcean:
    def __init__(self):
        self.token = os.environ["DIGITALOCEAN_ACCESS_TOKEN"]
        if not self.token:
            raise RuntimeError("DIGITALOCEAN_ACCESS_TOKEN is empty")

    def request(self, method, path, data=None):
        # POST is deliberately not retried: a lost response may still create a host.
        # The durable unique name and repo tag allow cleanup to recover its ID.
        request = urllib.request.Request(
            API + path, data=None if data is None else json.dumps(data).encode(),
            headers={"Authorization": "Bearer " + self.token, "Content-Type": "application/json"},
            method=method,
        )
        attempts = 1 if method == "POST" else 4
        for attempt in range(attempts):
            try:
                with urllib.request.urlopen(request, timeout=30) as response:
                    body = response.read()
                    return json.loads(body) if body else None
            except urllib.error.HTTPError as error:
                if error.code == 404 and method in ("GET", "DELETE"):
                    return None
                if error.code not in (429, 500, 502, 503, 504) or attempt + 1 == attempts:
                    raise RuntimeError(f"DigitalOcean {method} {path}: HTTP {error.code}") from None
            except (urllib.error.URLError, TimeoutError):
                if attempt + 1 == attempts:
                    raise RuntimeError(f"DigitalOcean {method} {path}: connection failed") from None
            time.sleep(2 ** attempt)

    def pages(self, path, key):
        result = []
        for page in range(1, 101):
            sep = '&' if '?' in path else '?'
            value = self.request("GET", f"{path}{sep}per_page=200&page={page}")
            if value is None:
                return result
            result.extend(value[key])
            if not value.get("links", {}).get("pages", {}).get("next"):
                return result
        raise RuntimeError("DigitalOcean pagination exceeded limit")


def owner_tag():
    repo = os.environ["GITHUB_REPOSITORY"]
    return "reproto-bench-" + hashlib.sha256(repo.encode()).hexdigest()[:16]


def droplets(api):
    return api.pages("/droplets?tag_name=" + urllib.parse.quote(owner_tag()), "droplets")


def destroy(api, droplet):
    ident = int(droplet["id"])
    # Only callers that have matched both the repository tag and run name get here.
    api.request("DELETE", f"/droplets/{ident}")
    deadline = time.monotonic() + 180
    while time.monotonic() < deadline:
        if api.request("GET", f"/droplets/{ident}") is None:
            print(f"Destroyed benchmark droplet {ident}", flush=True)
            return
        time.sleep(5)
    raise RuntimeError(f"Droplet {ident} deletion is not confirmed; check DigitalOcean")


def cleanup(api, name):
    errors = []
    try:
        for droplet in droplets(api):
            if droplet["name"] == name and owner_tag() in droplet["tags"]:
                try:
                    destroy(api, droplet)
                except Exception as error:
                    errors.append(str(error))
    except Exception as error:
        errors.append(str(error))
    for key in api.pages("/account/keys", "ssh_keys"):
        if key["name"] == name:
            try:
                api.request("DELETE", f'/account/keys/{int(key["id"])}')
            except Exception as error:
                errors.append(str(error))
    if errors:
        raise RuntimeError("Cleanup failed: " + "; ".join(errors))


def janitor(api):
    now = time.time()
    prefix = owner_tag() + "-"
    names = set()
    for droplet in droplets(api):
        created = datetime.datetime.fromisoformat(droplet["created_at"].replace("Z", "+00:00")).timestamp()
        if droplet["name"].startswith(prefix) and owner_tag() in droplet["tags"] and now - created > TTL:
            names.add(droplet["name"])
    for key in api.pages("/account/keys", "ssh_keys"):
        if key["name"].startswith(prefix):
            timestamp = key["name"].removeprefix(prefix).split("-", 1)[0]
            if timestamp.isdigit() and now - int(timestamp) > TTL:
                names.add(key["name"])
    for name in sorted(names):
        cleanup(api, name)


def command(args, **kwargs):
    return subprocess.run(args, check=True, timeout=kwargs.pop("timeout", 120), **kwargs)


def run(api, bundle, destination, state):
    region = os.environ.get("DO_REGION", "nyc3")
    size_slug = os.environ.get("DO_SIZE", "c-4")
    if not re.fullmatch(r"c-[0-9]+(?:-intel)?", size_slug):
        raise RuntimeError("Use a dedicated CPU-Optimized c-N plan (shared CPUs are rejected)")
    sizes = api.pages("/sizes", "sizes")
    size = next((s for s in sizes if s["slug"] == size_slug), None)
    if size is None or not size["available"] or region not in size["regions"]:
        raise RuntimeError("Dedicated size unavailable in requested region")
    if not 0 < float(size["price_hourly"]) <= 0.50:
        raise RuntimeError("Dedicated plan exceeds the $0.50/hour rate ceiling")
    name = f'{owner_tag()}-{int(time.time())}-{os.environ["GITHUB_RUN_ID"]}-{os.environ.get("GITHUB_RUN_ATTEMPT", "1")}'
    state.mkdir(parents=True, exist_ok=True, mode=0o700)
    state.chmod(0o700)
    (state / "receipt.json").write_text(json.dumps({"name": name}))
    destination.mkdir(parents=True, exist_ok=True)
    for filename in ("trials.json", "environment.json", "instructions.jsonl", "runner.log"):
        (destination / filename).unlink(missing_ok=True)
    client_key, host_key = state / "client", state / "host"
    for key in (client_key, host_key):
        command(["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-f", str(key)])
    try:
        key = api.request("POST", "/account/keys", {"name": name, "public_key": client_key.with_suffix(".pub").read_text()})["ssh_key"]
        cloud_config = {"ssh_pwauth": False, "disable_root": False, "ssh_keys": {
            "ed25519_private": host_key.read_text(), "ed25519_public": host_key.with_suffix(".pub").read_text(),
        }}
        created = api.request("POST", "/droplets", {"name": name, "region": region, "size": size_slug,
            "image": "ubuntu-24-04-x64", "ssh_keys": [key["id"]], "tags": [owner_tag()],
            "backups": False, "monitoring": False, "with_droplet_agent": False,
            "user_data": "#cloud-config\n" + json.dumps(cloud_config)})["droplet"]
        ident = int(created["id"])
        receipt = {"name": name, "id": ident, "region": region, "size": size_slug,
                   "price_hourly": size["price_hourly"], "image": "ubuntu-24-04-x64"}
        (state / "receipt.json").write_text(json.dumps(receipt))
        (destination / "droplet.json").write_text(json.dumps(receipt, indent=2))
        print(f'Benchmark droplet {ident}: {size_slug}, {region}, ${size["price_hourly"]}/hour rate', flush=True)
        deadline = time.monotonic() + 600
        address = None
        while time.monotonic() < deadline:
            current = api.request("GET", f"/droplets/{ident}")["droplet"]
            for network in current["networks"]["v4"]:
                if current["status"] == "active" and network["type"] == "public":
                    address = str(ipaddress.IPv4Address(network["ip_address"]))
            if address:
                break
            time.sleep(5)
        if not address:
            raise RuntimeError("Droplet did not become active within ten minutes")
        known_hosts = state / "known_hosts"
        known_hosts.write_text(address + " " + host_key.with_suffix(".pub").read_text())
        options = ["-i", str(client_key), "-o", "BatchMode=yes", "-o", "StrictHostKeyChecking=yes",
                   "-o", f"UserKnownHostsFile={known_hosts}", "-o", "ConnectTimeout=10", "-o", "ServerAliveInterval=15",
                   "-o", "ServerAliveCountMax=3", "-o", "IdentitiesOnly=yes"]
        ssh = ["ssh", *options, "root@" + address]
        while time.monotonic() < deadline:
            result = subprocess.run([*ssh, "true"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=20)
            if result.returncode == 0:
                break
            time.sleep(5)
        else:
            raise RuntimeError("SSH readiness timed out")
        command([*ssh, "cloud-init status --wait"], timeout=240)
        command([*ssh, "apt-get update && apt-get install --no-install-recommends -y valgrind"], timeout=300)
        command([*ssh, "mkdir -p /root/bench /root/results"])
        archive = state / "bundle.tar.gz"
        with tarfile.open(archive, "w:gz") as tar:
            for path in sorted(bundle.iterdir()):
                if not path.is_file() or path.is_symlink():
                    raise RuntimeError("Unexpected bundle entry")
                tar.add(path, arcname=path.name)
        command(["scp", *options, str(archive), f"root@{address}:/root/bundle.tar.gz"], timeout=300)
        # Matching Ubuntu versions preserve glibc compatibility. Only runtime tools
        # and precompiled binaries are needed; no Cargo/Rust/compiler on this host.
        command([*ssh, "tar -xzf /root/bundle.tar.gz -C /root/bench && chmod +x /root/bench/native /root/bench/grpc /root/bench/websocket /root/bench/capnp_cpp /root/bench/capnp-reference /root/bench/driver /root/bench/hot_paths /root/bench/gungraun-runner && timeout --kill-after=10s 20m /root/bench/driver compare /root/bench /root/results > /root/results/runner.log 2>&1"], timeout=1250)
        for filename in ("trials.json", "environment.json", "instructions.jsonl", "runner.log"):
            command(["scp", *options, f"root@{address}:/root/results/{filename}", str(destination / filename)])
    finally:
        cleanup(api, name)
        # Keys are outside uploaded artifacts; remove locally after destruction.
        for key in (client_key, host_key):
            key.unlink(missing_ok=True)
            key.with_suffix(".pub").unlink(missing_ok=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=("run", "cleanup", "janitor"))
    parser.add_argument("--bundle", type=Path)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--state", type=Path, required=True)
    args = parser.parse_args()
    api = DigitalOcean()
    if args.action == "janitor":
        janitor(api)
    elif args.action == "cleanup":
        receipt = args.state / "receipt.json"
        if receipt.exists():
            cleanup(api, json.loads(receipt.read_text())["name"])
    else:
        if args.bundle is None or args.output is None:
            parser.error("run requires --bundle and --output")
        run(api, args.bundle.resolve(), args.output.resolve(), args.state.resolve())


if __name__ == "__main__":
    def interrupted(signum, frame):
        raise SystemExit(128 + signum)
    signal.signal(signal.SIGTERM, interrupted)
    signal.signal(signal.SIGINT, interrupted)
    try:
        main()
    except Exception as error:
        print(f"Benchmark infrastructure failed: {error}", file=sys.stderr)
        sys.exit(1)
