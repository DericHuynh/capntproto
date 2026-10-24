use super::*;
use std::cell::RefCell;
#[derive(Default)]
struct Fake {
    hosts: RefCell<Vec<Value>>,
    keys: RefCell<Vec<Value>>,
    calls: RefCell<Vec<(String, String, Value)>>,
    mode: &'static str,
}
impl Api for Fake {
    fn wait_for_key_visibility(&self, cancel: &Cancellation) -> Result<()> {
        ensure!(!cancel.cancelled(), "operation interrupted");
        Ok(())
    }
    fn pages(&self, _: &str, key: &str) -> Result<Vec<Value>> {
        match key {
            "droplets" => Ok(self.hosts.borrow().clone()),
            "ssh_keys" => Ok(self.keys.borrow().clone()),
            "sizes" => {
                let mut sizes = sizes();
                if self.mode == "unavailable" {
                    sizes[0]["available"] = json!(false);
                }
                if self.mode == "expensive" {
                    sizes[0]["price_hourly"] = json!(0.51);
                }
                Ok(sizes)
            }
            _ => bail!("unexpected page"),
        }
    }
    fn request(&self, method: &str, path: &str, data: Option<&Value>) -> Result<Option<Value>> {
        self.calls.borrow_mut().push((
            method.into(),
            path.into(),
            data.cloned().unwrap_or(Value::Null),
        ));
        match (method, path) {
            ("POST", "/account/keys") => {
                let key = json!({"id":9,"name":data.unwrap()["name"]});
                self.keys.borrow_mut().push(key.clone());
                Ok(Some(json!({"ssh_key":key})))
            }
            ("POST", "/droplets") => {
                let payload = data.unwrap();
                let key_rejection = self.mode == "key-stuck"
                    || self.mode == "key-missing"
                    || self.mode == "key-foreign"
                    || self.mode == "key-ambiguous"
                    || (self.mode == "key-delayed"
                        && self
                            .calls
                            .borrow()
                            .iter()
                            .filter(|(m, p, _)| m == "POST" && p == "/droplets")
                            .count()
                            == 1);
                if key_rejection {
                    return Err(ProviderError {
                        status: if self.mode == "key-ambiguous" {
                            500
                        } else {
                            422
                        },
                        code: "unprocessable_entity".into(),
                        message: "9 are invalid key identifiers for Droplet creation.".into(),
                        method: method.into(),
                        path: path.into(),
                    }
                    .into());
                }
                let d = json!({"id":7,"name":payload["name"],"tags":payload["tags"],"status":"active","networks":{"v4":[{"type":"public","ip_address":"192.0.2.1"}]}});
                if self.mode == "lost" {
                    self.hosts.borrow_mut().push(d);
                    bail!("Lost creation response");
                }
                if self.mode == "quota" || (payload["region"] == "nyc3" && self.mode == "capacity")
                {
                    return Err(ProviderError {
                        status: 422,
                        code: "unprocessable_entity".into(),
                        message: if self.mode == "quota" {
                            "Droplet limit exceeded."
                        } else {
                            "Size is not available in this region."
                        }
                        .into(),
                        method: method.into(),
                        path: path.into(),
                    }
                    .into());
                }
                self.hosts.borrow_mut().push(d.clone());
                Ok(Some(json!({"droplet":d})))
            }
            ("DELETE", p) if p.starts_with("/droplets/") => {
                if self.mode == "refuse" {
                    bail!("Deletion refused");
                }
                self.hosts
                    .borrow_mut()
                    .retain(|d| format!("/droplets/{}", d["id"]) != p);
                Ok(None)
            }
            ("GET", p) if p.starts_with("/droplets/") => Ok(self
                .hosts
                .borrow()
                .iter()
                .find(|d| format!("/droplets/{}", d["id"]) == p)
                .map(|d| json!({"droplet":d}))),
            ("DELETE", p) if p.starts_with("/account/keys/") => {
                self.keys
                    .borrow_mut()
                    .retain(|d| format!("/account/keys/{}", d["id"]) != p);
                Ok(None)
            }
            ("GET", p) if p.starts_with("/account/keys/") => Ok(self
                .keys
                .borrow()
                .iter()
                .find(|k| format!("/account/keys/{}", k["id"]) == p)
                .map(|key| json!({"ssh_key": key}))),
            _ => bail!("unexpected request"),
        }
    }
}
#[test]
fn new_key_visibility_retry_is_narrow_bounded_and_keeps_one_host() {
    let owner = tag("test/capntproto");
    let name = format!("{owner}-42-1");
    for (mode, posts) in [
        ("key-delayed", 2),
        ("key-stuck", 4),
        ("key-missing", 1),
        ("key-foreign", 1),
        ("key-ambiguous", 1),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let api = Fake {
            mode,
            ..Default::default()
        };
        if mode != "key-missing" {
            api.keys.borrow_mut().push(
                json!({"id":9,"name":if mode == "key-foreign" { "unrelated" } else { &name }}),
            );
        }
        let plan = select(sizes(), "auto", "auto", &BTreeSet::new()).unwrap();
        let result = create_host(
            &api,
            json!({"name":name,"tags":[owner],"ssh_keys":[9]}),
            plan,
            dir.path(),
            &owner,
            &Cancellation::default(),
            |_| panic!("key rejection is not a capacity fallback"),
        );
        assert_eq!(result.is_ok(), mode == "key-delayed", "{mode}: {result:?}");
        assert_eq!(
            api.calls
                .borrow()
                .iter()
                .filter(|(m, p, _)| m == "POST" && p == "/droplets")
                .count(),
            posts
        );
        assert_eq!(api.hosts.borrow().len(), usize::from(mode == "key-delayed"));
        let journal = crate::read_json(dir.path().join("provisioning.json")).unwrap();
        assert_eq!(journal.as_array().unwrap().len(), posts);
    }
}
fn sizes() -> Vec<Value> {
    vec![
        json!({"slug":"c-4","available":true,"price_hourly":0.1,"vcpus":4,"regions":["nyc3","nyc1"]}),
    ]
}
fn provision(api: &Fake, out: &Path, region: &str) -> Result<(Value, Plan)> {
    let owner = tag("test/capntproto");
    let plan = select(sizes(), "auto", region, &BTreeSet::new())?;
    create_host(
        api,
        json!({"name":format!("{owner}-42-1"),"tags":[owner],"image":"ubuntu-24-04-x64"}),
        plan,
        out,
        &owner,
        &Cancellation::default(),
        |excluded| select(sizes(), "auto", region, excluded),
    )
}
#[test]
fn capacity_fallback_is_bounded_and_journaled() {
    let dir = tempfile::tempdir().unwrap();
    let api = Fake {
        mode: "capacity",
        ..Default::default()
    };
    let (host, plan) = provision(&api, dir.path(), "auto").unwrap();
    assert_eq!(host["id"], 7);
    assert_eq!(plan.region, "nyc1");
    assert_eq!(api.hosts.borrow().len(), 1);
    let journal = crate::read_json(dir.path().join("provisioning.json")).unwrap();
    assert_eq!(journal[0]["status"], "rejected");
    assert_eq!(journal[1]["status"], "created");
    assert_eq!(api.calls.borrow().len(), 2);
}
#[test]
fn quota_explicit_region_and_contradictory_state_stop_creation() {
    let dir = tempfile::tempdir().unwrap();
    let api = Fake {
        mode: "quota",
        ..Default::default()
    };
    assert!(provision(&api, dir.path(), "auto").is_err());
    assert_eq!(api.calls.borrow().len(), 1);
    let api = Fake {
        mode: "capacity",
        ..Default::default()
    };
    assert!(provision(&api, dir.path(), "nyc3").is_err());
    assert_eq!(api.calls.borrow().len(), 1);
    let owner = tag("test/capntproto");
    api.calls.borrow_mut().clear();
    api.hosts
        .borrow_mut()
        .push(json!({"id":7,"name":format!("{owner}-42-1"),"tags":[owner]}));
    assert!(provision(&api, dir.path(), "auto")
        .unwrap_err()
        .to_string()
        .contains("refusing another POST"));
    assert_eq!(api.calls.borrow().len(), 1);
}
#[test]
fn lost_creation_response_is_not_retried_and_cleanup_recovers_it() {
    let dir = tempfile::tempdir().unwrap();
    let api = Fake {
        mode: "lost",
        ..Default::default()
    };
    assert!(provision(&api, dir.path(), "auto").is_err());
    assert_eq!(api.calls.borrow().len(), 1);
    assert_eq!(api.hosts.borrow().len(), 1);
    let journal = crate::read_json(dir.path().join("provisioning.json")).unwrap();
    assert_eq!(journal[0]["status"], "unknown");
    let owner = tag("test/capntproto");
    cleanup(&api, &format!("{owner}-42-1"), &owner).unwrap();
    assert!(api.hosts.borrow().is_empty());
    cleanup(&api, &format!("{owner}-42-1"), &owner).unwrap();
}
#[test]
fn janitor_preserves_active_hosts_and_foreign_resources() {
    let owner = tag("test/capntproto");
    let expired = format!("{owner}-1-1");
    let active = format!("{owner}-{}-2", chrono::Utc::now().timestamp());
    let api = Fake::default();
    api.hosts.borrow_mut().extend([json!({"id":1,"name":expired,"tags":[owner],"created_at":"2000-01-01T00:00:00Z"}),json!({"id":2,"name":active,"tags":[owner],"created_at":"2100-01-01T00:00:00Z"}),json!({"id":3,"name":"unrelated","tags":["production"],"created_at":"2000-01-01T00:00:00Z"})]);
    api.keys.borrow_mut().extend([
        json!({"id":1,"name":expired}),
        json!({"id":2,"name":active}),
    ]);
    janitor(&api, &owner).unwrap();
    assert_eq!(
        api.hosts
            .borrow()
            .iter()
            .map(|d| d["id"].clone())
            .collect::<Vec<_>>(),
        vec![json!(2), json!(3)]
    );
    assert_eq!(api.keys.borrow().len(), 1);
    let api = Fake {
        mode: "refuse",
        ..api
    };
    assert!(cleanup(&api, &active, &owner)
        .unwrap_err()
        .to_string()
        .contains("Deletion refused"));
    assert_eq!(api.hosts.borrow().len(), 2);
}

struct FakeCommands {
    timeout: bool,
    downloads: RefCell<Vec<String>>,
}
impl Commands for FakeCommands {
    fn command(
        &self,
        program: &str,
        args: &[String],
        seconds: u64,
        _: &Cancellation,
    ) -> Result<()> {
        if program == "ssh-keygen" {
            let path = Path::new(args.last().unwrap());
            crate::write(path, "fake-private-key")?;
            crate::write(path.with_extension("pub"), "ssh-ed25519 fake-public-key\n")?;
        } else if program == "ssh" && args.last().unwrap().contains("pipefail") {
            let remote = args.last().unwrap();
            ensure!(
                remote.contains("tee /root/results/runner.log"),
                "missing live progress"
            );
            ensure!(
                remote.contains("timeout --kill-after=10s 20m"),
                "unbounded remote run"
            );
            ensure!(seconds == 1250, "unexpected local deadline");
            if self.timeout {
                bail!("fake measurement deadline");
            }
        } else if program == "scp" && args[args.len() - 2].contains(":/root/results/") {
            let file = args[args.len() - 2].rsplit('/').next().unwrap();
            self.downloads.borrow_mut().push(file.into());
            if self.timeout && file == "instructions.jsonl" {
                bail!("fake missing partial result");
            }
            crate::write(args.last().unwrap(), "partial evidence")?;
        }
        Ok(())
    }
    fn ready(&self, args: &[String], _: &Cancellation) -> bool {
        assert!(args.iter().any(|a| a == "StrictHostKeyChecking=yes"));
        let known = args
            .iter()
            .find_map(|a| a.strip_prefix("UserKnownHostsFile="))
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(known).unwrap(),
            "192.0.2.1 ssh-ed25519 fake-public-key\n"
        );
        true
    }
}

#[test]
fn full_benchmark_lifecycle_preserves_partial_results_and_always_cleans_up() {
    // nextest gives this environment-sensitive command test its own process.
    for (name, value) in [
        ("GITHUB_RUN_ID", "42"),
        ("GITHUB_RUN_ATTEMPT", "1"),
        ("DO_SIZE", "auto"),
        ("DO_REGION", "auto"),
    ] {
        std::env::set_var(name, value);
    }
    let owner = tag("test/capntproto");
    for mode in [
        "success",
        "timeout",
        "lost",
        "refuse",
        "unavailable",
        "expensive",
        "key-delayed",
        "key-stuck",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let bundle = dir.path().join("bundle");
        crate::write(bundle.join("native"), "fake precompiled binary").unwrap();
        let output = dir.path().join("results");
        let state = dir.path().join("state");
        crate::write(output.join("trials.json"), "stale").unwrap();
        let api = Fake {
            mode,
            ..Default::default()
        };
        let commands = FakeCommands {
            timeout: mode == "timeout",
            downloads: RefCell::new(vec![]),
        };
        let result = benchmark(
            &api,
            &bundle,
            &output,
            &state,
            &owner,
            &Cancellation::default(),
            &commands,
        );
        assert_eq!(
            result.is_ok(),
            matches!(mode, "success" | "key-delayed"),
            "{mode}: {result:?}"
        );
        if ["unavailable", "expensive"].contains(&mode) {
            assert!(api.calls.borrow().is_empty());
            assert!(!output.join("trials.json").exists());
            assert_eq!(
                crate::read_json(output.join("capacity.json")).unwrap()["status"],
                "unavailable"
            );
        }
        if mode == "refuse" {
            assert!(result.unwrap_err().to_string().contains("Deletion refused"));
            assert!(
                state.join("client").exists(),
                "keep credentials until cleanup is confirmed"
            );
        } else {
            assert!(api.hosts.borrow().is_empty(), "leaked {mode} host");
            assert!(api.keys.borrow().is_empty(), "leaked {mode} SSH key");
            assert!(!state.join("client").exists());
            assert!(!state.join("host").exists());
        }
        if mode == "timeout" {
            assert_eq!(
                *commands.downloads.borrow(),
                [
                    "runner.log",
                    "trials.json",
                    "environment.json",
                    "instructions.jsonl"
                ]
            );
            assert_eq!(
                std::fs::read_to_string(output.join("runner.log")).unwrap(),
                "partial evidence"
            );
            assert!(!output.join("instructions.jsonl").exists());
        }
    }
}
