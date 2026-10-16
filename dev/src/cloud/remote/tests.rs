use super::*;
use std::cell::{Cell, RefCell};

#[test]
fn workflow_identity_uses_the_override_or_current_process_without_global_env_changes() {
    assert_eq!(workflow_run_id(None), std::process::id().to_string());
    assert_eq!(workflow_run_id(Some("1234".into())), "1234");
    // Invalid configured identities must still reach run()'s numeric check.
    assert_eq!(workflow_run_id(Some("invalid".into())), "invalid");
}

#[derive(Default)]
struct ApiFixture {
    hosts: RefCell<Vec<Value>>,
    keys: RefCell<Vec<Value>>,
    deleted: RefCell<Vec<String>>,
    fail_delete: bool,
}
impl Api for ApiFixture {
    fn pages(&self, _: &str, key: &str) -> Result<Vec<Value>> {
        Ok(match key {
            "droplets" => self.hosts.borrow().clone(),
            "ssh_keys" => self.keys.borrow().clone(),
            _ => bail!("unexpected page"),
        })
    }
    fn request(&self, method: &str, path: &str, _: Option<&Value>) -> Result<Option<Value>> {
        if method == "DELETE" {
            self.deleted.borrow_mut().push(path.into());
            if self.fail_delete && path == "/droplets/1" {
                bail!("injected deletion failure");
            }
            self.hosts
                .borrow_mut()
                .retain(|d| format!("/droplets/{}", d["id"]) != path);
            return Ok(None);
        }
        if method == "GET" {
            return Ok(None);
        }
        bail!("unexpected request")
    }
}
#[test]
fn cleanup_attempts_both_hosts_and_keys_after_partial_failure() {
    let owner = tag("test/pair");
    let server = format!("{owner}-1-server");
    let client = format!("{owner}-1-client");
    let api = ApiFixture {
        fail_delete: true,
        ..Default::default()
    };
    api.hosts.borrow_mut().extend([
        json!({"id":1,"name":server,"tags":[owner]}),
        json!({"id":2,"name":client,"tags":[owner]}),
        json!({"id":3,"name":"unrelated","tags":[owner]}),
    ]);
    api.keys.borrow_mut().extend([
        json!({"id":11,"name":server}),
        json!({"id":12,"name":client}),
    ]);
    assert!(cleanup_receipt(&api, &json!({"names":[server,client]}), &owner).is_err());
    let deleted = api.deleted.borrow();
    for path in [
        "/droplets/1",
        "/droplets/2",
        "/account/keys/11",
        "/account/keys/12",
    ] {
        assert!(deleted.iter().any(|v| v == path), "{path}");
    }
    assert!(!deleted.iter().any(|v| v == "/droplets/3"));
}
#[test]
fn requires_real_private_addresses_and_vpc_identity() {
    let d = json!({"vpc_uuid":"01234567-89ab-cdef-0123-456789abcdef","networks":{"v4":[{"type":"public","ip_address":"192.0.2.1"},{"type":"private","ip_address":"10.0.0.1"}]}});
    assert!(addresses(&d).is_ok());
    let mut missing = d.clone();
    missing["networks"]["v4"] = json!([{"type":"public","ip_address":"192.0.2.1"}]);
    assert!(addresses(&missing).is_err());
    let mut loopback = d.clone();
    loopback["networks"]["v4"][1]["ip_address"] = json!("127.0.0.1");
    assert!(addresses(&loopback).is_err());
    let mut injection = d;
    injection["vpc_uuid"] = json!("x;cmd");
    assert!(addresses(&injection).is_err());
}
#[test]
fn active_hosts_wait_for_network_metadata_but_reject_invalid_addresses() {
    let complete = json!({"status":"active","vpc_uuid":"01234567-89ab-cdef-0123-456789abcdef","networks":{"v4":[{"type":"public","ip_address":"192.0.2.1"},{"type":"private","ip_address":"10.0.0.1"}]}});
    for path in [
        "/networks",
        "/networks/v4/0/ip_address",
        "/networks/v4/1/ip_address",
        "/vpc_uuid",
    ] {
        for missing in [Value::Null, json!("")] {
            let mut host = complete.clone();
            *host.pointer_mut(path).unwrap() = missing;
            assert_eq!(ready_addresses(&host).unwrap(), None);
        }
    }
    let mut pending = complete.clone();
    pending["status"] = json!("new");
    assert_eq!(ready_addresses(&pending).unwrap(), None);
    assert_eq!(
        ready_addresses(&complete).unwrap(),
        Some(addresses(&complete).unwrap())
    );
    for (path, invalid) in [
        ("/networks/v4/0/ip_address", json!("invalid")),
        ("/networks/v4/1/ip_address", json!("127.0.0.1")),
        ("/vpc_uuid", json!(17)),
    ] {
        let mut host = complete.clone();
        *host.pointer_mut(path).unwrap() = invalid;
        assert!(ready_addresses(&host).is_err());
    }
}
#[test]
fn artifact_unpack_rejects_symlinks() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("input.gz");
    let mut tar = tar::Builder::new(flate2::write::GzEncoder::new(
        fs::File::create(&path).unwrap(),
        flate2::Compression::default(),
    ));
    let mut header = tar::Header::new_gnu();
    header.set_entry_type(tar::EntryType::Symlink);
    header.set_size(0);
    header.set_mode(0o600);
    header.set_link_name("/tmp/not-an-artifact").unwrap();
    header.set_cksum();
    tar.append_data(&mut header, "bad-link", std::io::empty())
        .unwrap();
    tar.into_inner().unwrap().finish().unwrap();
    assert!(unpack(&path, dir.path()).is_err());
    assert!(!dir.path().join("bad-link").exists());
}

#[derive(Default)]
struct PairApi {
    hosts: RefCell<Vec<Value>>,
    keys: RefCell<Vec<Value>>,
    creates: RefCell<Vec<Value>>,
    mode: &'static str,
    delayed_metadata: Cell<bool>,
}
impl Api for PairApi {
    fn pages(&self, _: &str, key: &str) -> Result<Vec<Value>> {
        Ok(match key {
            "droplets" => self.hosts.borrow().clone(),
            "ssh_keys" => self.keys.borrow().clone(),
            "sizes" => vec![
                json!({"slug":"c-4","price_hourly":0.125,"available":true,"regions":["nyc3"],"vcpus":4}),
            ],
            _ => bail!("unexpected page"),
        })
    }
    fn request(&self, method: &str, path: &str, data: Option<&Value>) -> Result<Option<Value>> {
        match (method, path) {
            ("POST", "/account/keys") => {
                let key = json!({"id":self.keys.borrow().len()+10,"name":data.unwrap()["name"]});
                self.keys.borrow_mut().push(key.clone());
                Ok(Some(json!({"ssh_key":key})))
            }
            ("POST", "/droplets") => {
                let data = data.unwrap().clone();
                self.creates.borrow_mut().push(data.clone());
                let id = self.creates.borrow().len();
                if id == 2 && self.mode == "capacity" {
                    return Err(ProviderError {
                        status: 422,
                        code: "unprocessable_entity".into(),
                        message: "Insufficient capacity".into(),
                        method: method.into(),
                        path: path.into(),
                    }
                    .into());
                }
                let host = json!({"id":id,"name":data["name"],"tags":data["tags"],"status":"active","vpc_uuid":if id==2&&self.mode=="foreign-vpc" {"11111111-1111-1111-1111-111111111111"} else {"00000000-0000-0000-0000-000000000001"},"networks":{"v4":[{"type":"public","ip_address":format!("192.0.2.{id}")},{"type":"private","ip_address":format!("10.0.0.{id}")}]}});
                self.hosts.borrow_mut().push(host.clone());
                if id == 2 && self.mode == "lost" {
                    bail!("lost creation response");
                }
                Ok(Some(json!({"droplet":host})))
            }
            ("GET", p) if p.starts_with("/droplets/") => Ok(self
                .hosts
                .borrow()
                .iter()
                .find(|d| format!("/droplets/{}", d["id"]) == p)
                .map(|d| {
                    let mut host = d.clone();
                    if p == "/droplets/2" && self.delayed_metadata.replace(false) {
                        host["networks"]["v4"] =
                            json!([{"type":"public","ip_address":"192.0.2.2"}]);
                    }
                    json!({"droplet":host})
                })),
            ("DELETE", p) if p.starts_with("/droplets/") => {
                self.hosts
                    .borrow_mut()
                    .retain(|d| format!("/droplets/{}", d["id"]) != p);
                Ok(None)
            }
            ("DELETE", p) if p.starts_with("/account/keys/") => {
                self.keys
                    .borrow_mut()
                    .retain(|d| format!("/account/keys/{}", d["id"]) != p);
                Ok(None)
            }
            _ => bail!("unexpected API operation"),
        }
    }
}
#[derive(Default)]
struct PairCommands(RefCell<Vec<String>>);
impl Commands for PairCommands {
    fn ready(&self, _: &[String], _: &Cancellation) -> bool {
        true
    }
    fn command(&self, program: &str, args: &[String], _: u64, _: &Cancellation) -> Result<()> {
        self.0
            .borrow_mut()
            .push(format!("{program} {}", args.join(" ")));
        if program == "ssh-keygen" {
            let key = Path::new(args.last().unwrap());
            crate::write(key, "FAKE PRIVATE KEY")?;
            crate::write(key.with_extension("pub"), "ssh-ed25519 FAKE PUBLIC KEY\n")?;
        }
        if args.iter().any(|a| a.contains("workflow-client")) {
            bail!("injected measurement failure");
        }
        Ok(())
    }
}

struct CompletedCommands {
    commands: PairCommands,
    manifest: Value,
    fail_at: &'static str,
}
impl Commands for CompletedCommands {
    fn ready(&self, _: &[String], _: &Cancellation) -> bool {
        true
    }
    fn command(
        &self,
        program: &str,
        args: &[String],
        timeout: u64,
        cancel: &Cancellation,
    ) -> Result<()> {
        if !self.fail_at.is_empty() && args.iter().any(|arg| arg.contains(self.fail_at)) {
            bail!("injected {} failure", self.fail_at);
        }
        if args.iter().any(|arg| arg.contains("workflow-client")) {
            self.commands.0.borrow_mut().push(args.join(" "));
            return Ok(());
        }
        self.commands.command(program, args, timeout, cancel)
    }
    fn download(
        &self,
        _: &[String],
        address: &str,
        output: &Path,
        file: &str,
        _: u64,
        _: &Cancellation,
    ) -> Result<()> {
        self.commands
            .0
            .borrow_mut()
            .push(format!("download {address} {file}"));
        let path = output.join(file);
        fs::create_dir_all(path.parent().unwrap())?;
        if file != "remote.tar.gz" {
            return crate::write(path, "synthetic lifecycle fixture\n");
        }
        let server = address == "192.0.2.1";
        let role = if server { "server" } else { "client" };
        let mut environment = json!({
            "role": role, "workflow_version": report::tests::VERSION,
            "smoke": false, "boot_id": role, "manifest": self.manifest,
            "server_address": "10.0.0.1", "bind_address": "10.0.0.1"
        });
        environment["server_resources"] = report::tests::resources();
        let mut files = vec![(
            format!("{role}-environment.json"),
            serde_json::to_vec(&environment)?,
        )];
        if server {
            files.push(("server-stopped".into(), b"ok\n".to_vec()));
        } else {
            files.push((
                "workflow-trials.json".into(),
                serde_json::to_vec(&report::tests::trials())?,
            ));
        }
        let mut archive = tar::Builder::new(flate2::write::GzEncoder::new(
            fs::File::create(path)?,
            flate2::Compression::default(),
        ));
        for (name, bytes) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(0o600);
            header.set_cksum();
            archive.append_data(&mut header, name, bytes.as_slice())?;
        }
        archive.into_inner()?.finish()?;
        Ok(())
    }
}

fn fixture_bundle(path: &Path) -> Value {
    fs::create_dir(path).unwrap();
    let mut binaries = serde_json::Map::new();
    for name in [
        "driver",
        "capnp-reference",
        "capnp-rust",
        "tcp",
        "native",
        "grpc",
        "websocket",
        "capnp_cpp",
        "hot_paths",
        "gungraun-runner",
    ] {
        crate::write(path.join(name), "fake binary").unwrap();
        binaries.insert(name.into(), json!(crate::hash("fake binary")));
    }
    let manifest = json!({"binaries": binaries});
    crate::write_json(path.join("manifest.json"), &manifest).unwrap();
    manifest
}

#[test]
fn complete_remote_campaign_runs_both_controls_and_salvages_failed_portable_control() {
    // Synthetic archives exercise orchestration only. These are never timing
    // evidence and no process, provider API or paid host is contacted.
    for fail_at in [
        "",
        "compare-portable",
        "compare /root/bench",
        "workflow-measure",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let bundle = dir.path().join("bundle");
        let commands = CompletedCommands {
            commands: PairCommands::default(),
            manifest: fixture_bundle(&bundle),
            fail_at,
        };
        let api = PairApi {
            delayed_metadata: Cell::new(fail_at.is_empty()),
            ..Default::default()
        };
        let output = dir.path().join("output");
        let state = dir.path().join("state");
        let result = run(
            &api,
            &bundle,
            &output,
            &state,
            &tag("test/complete-pair"),
            &Cancellation::default(),
            &commands,
            true,
        );
        assert_eq!(result.is_ok(), fail_at.is_empty(), "{fail_at}: {result:?}");
        assert!(api.hosts.borrow().is_empty());
        assert!(api.keys.borrow().is_empty());
        assert!(!state.join("client/host").exists());
        assert!(output.join("remote/summary.json").is_file());
        for file in [
            "portable-loopback/trials.json",
            "portable-loopback/environment.json",
            "portable-loopback/summary.json",
            "portable-loopback/README.md",
            "portable-loopback.log",
        ] {
            assert_eq!(
                fs::read_to_string(output.join(file)).unwrap(),
                "synthetic lifecycle fixture\n"
            );
            assert!(commands
                .commands
                .0
                .borrow()
                .iter()
                .any(|call| call == &format!("download 192.0.2.2 {file}")));
        }
        if fail_at.is_empty() {
            let calls = commands.commands.0.borrow();
            let legacy = calls
                .iter()
                .position(|call| {
                    call.contains("CAPNTPROTO_BENCH_PORTABLE=0")
                        && call.contains("compare /root/bench")
                })
                .unwrap();
            let portable = calls
                .iter()
                .position(|call| call.contains("compare-portable /root/bench"))
                .unwrap();
            assert!(legacy < portable);
            let trace = calls
                .iter()
                .position(|call| call.contains("strace --version"))
                .unwrap();
            assert!(
                portable < trace,
                "tracing must follow all untraced controls"
            );
            assert_eq!(
                calls
                    .iter()
                    .filter(|call| call.contains("workflow-measure"))
                    .count(),
                15
            );
            assert!(output
                .join("diagnostics/client/io-trace/pipelined-read-cpp-client.strace")
                .is_file());
            assert!(output.join("trials.json").is_file());
        }
        if fail_at == "workflow-measure" {
            let calls = commands.commands.0.borrow();
            assert!(calls.iter().any(|call| call.contains("kill -TERM")));
            assert!(output
                .join("diagnostics/server/io-trace/unary-empty-portable-server.strace")
                .is_file());
            assert!(output.join("portable-loopback/trials.json").is_file());
        }
    }
}

#[test]
fn pair_lifecycle_cleans_up_after_second_creation_and_measurement_failures() {
    // No API, SSH, paid resource or real key is used by this fault campaign.
    for mode in ["capacity", "lost", "foreign-vpc", "measurement"] {
        let dir = tempfile::tempdir().unwrap();
        let bundle = dir.path().join("bundle");
        fs::create_dir(&bundle).unwrap();
        let mut binaries = serde_json::Map::new();
        for name in [
            "driver",
            "capnp-reference",
            "capnp-rust",
            "tcp",
            "native",
            "grpc",
            "websocket",
            "capnp_cpp",
            "hot_paths",
            "gungraun-runner",
        ] {
            crate::write(bundle.join(name), "fake binary").unwrap();
            binaries.insert(name.into(), json!(crate::hash("fake binary")));
        }
        crate::write_json(bundle.join("manifest.json"), &json!({"binaries":binaries})).unwrap();
        let api = PairApi {
            mode,
            ..Default::default()
        };
        let commands = PairCommands::default();
        let state = dir.path().join("state");
        let result = run(
            &api,
            &bundle,
            &dir.path().join("output"),
            &state,
            &tag("test/remote-pair"),
            &Cancellation::default(),
            &commands,
            false,
        );
        assert!(result.is_err(), "{mode}");
        assert!(api.hosts.borrow().is_empty(), "{mode}: leaked host");
        assert!(api.keys.borrow().is_empty(), "{mode}: leaked SSH key");
        let creates = api.creates.borrow();
        assert_eq!(
            creates.len(),
            2,
            "{mode}: ambiguous creation must not retry"
        );
        assert_eq!(creates[0]["region"], creates[1]["region"]);
        assert_eq!(creates[0]["size"], creates[1]["size"]);
        assert_eq!(
            creates[1]["vpc_uuid"],
            "00000000-0000-0000-0000-000000000001"
        );
        let receipt = crate::read_json(state.join("receipt.json")).unwrap();
        assert_eq!(receipt["names"].as_array().unwrap().len(), 2);
        assert!(!state.join("server/client").exists());
        assert!(!state.join("client/host").exists());
        if mode == "measurement" {
            let output = dir.path().join("output");
            let loopback_host = crate::read_json(output.join("droplet.json")).unwrap();
            let topology = crate::read_json(output.join("remote/topology.json")).unwrap();
            assert_eq!(loopback_host, topology["client"]);
            assert_ne!(loopback_host, topology["server"]);
            let calls = commands.0.borrow();
            assert!(
                calls
                    .iter()
                    .any(|c| c.contains(":/root/results/portable-loopback/trials.json")),
                "partial portable controls must be salvaged before cleanup"
            );
            assert!(calls
                .iter()
                .any(|c| c.contains("workflow-client /root/bench 10.0.0.1")));
            assert!(calls
                .iter()
                .any(|c| c.contains("-s 10.0.0.2/32 -d 10.0.0.1/32")));
        }
    }
}
