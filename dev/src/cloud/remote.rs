//! Two dedicated peers, one VPC, with independent receipts and unconditional cleanup.
use super::*;
use std::{fs, net::Ipv4Addr};
mod report;

struct Host {
    public: Ipv4Addr,
    private: Ipv4Addr,
    options: Vec<String>,
    plan: Plan,
    vpc: String,
    receipt: Value,
}
fn addresses(d: &Value) -> Result<(Ipv4Addr, Ipv4Addr, String)> {
    let ip = |kind| -> Result<Ipv4Addr> {
        d["networks"]["v4"]
            .as_array()
            .context("missing host network")?
            .iter()
            .find(|n| n["type"] == kind)
            .and_then(|n| n["ip_address"].as_str())
            .context("missing host address")?
            .parse()
            .context("invalid IPv4 address")
    };
    let public = ip("public")?;
    let private = ip("private")?;
    ensure!(
        private.is_private() && private != public,
        "missing distinct private VPC address"
    );
    let vpc = crate::string(d, "vpc_uuid")?;
    ensure!(
        regex(r"^[0-9a-fA-F-]{36}$").is_match(vpc),
        "invalid VPC identity"
    );
    Ok((public, private, vpc.to_owned()))
}
fn provision(
    api: &impl Api,
    commands: &impl Commands,
    paths: (&Path, &Path),
    name: &str,
    owner: &str,
    paired: Option<&Host>,
    cancel: &Cancellation,
) -> Result<Host> {
    let (state, output) = paths;
    fs::create_dir_all(state)?;
    fs::create_dir_all(output)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(state, fs::Permissions::from_mode(0o700))?;
    }
    let client = state.join("client");
    let host = state.join("host");
    for key in [&client, &host] {
        commands.command(
            "ssh-keygen",
            &[
                "-q".into(),
                "-t".into(),
                "ed25519".into(),
                "-N".into(),
                "".into(),
                "-f".into(),
                key.display().to_string(),
            ],
            120,
            cancel,
        )?;
    }
    let key = api.request("POST", "/account/keys", Some(&json!({"name":name,"public_key":fs::read_to_string(client.with_extension("pub"))?})))?.context("missing SSH key")?;
    let config = json!({"ssh_pwauth":false,"disable_root":false,"ssh_keys":{"ed25519_private":fs::read_to_string(&host)?,"ed25519_public":fs::read_to_string(host.with_extension("pub"))?}});
    let mut payload = json!({"name":name,"image":"ubuntu-24-04-x64","ssh_keys":[crate::number(&key["ssh_key"],"id")?],"tags":[owner],"backups":false,"monitoring":false,"with_droplet_agent":false,"user_data":format!("#cloud-config\n{config}")});
    let plan = if let Some(paired) = paired {
        payload["vpc_uuid"] = json!(paired.vpc);
        paired.plan.clone()
    } else {
        preflight(api, Some(output))?
    };
    let (created, plan) = create_host(api, payload, plan, output, owner, cancel, |excluded| {
        if let Some(paired) = paired {
            // Never silently move the second peer to another region or plan.
            select(
                api.pages("/sizes", "sizes")?,
                crate::string(&paired.plan.size, "slug")?,
                &paired.plan.region,
                excluded,
            )
        } else {
            select_plan(api, excluded)
        }
    })?;
    let id = crate::number(&created, "id")?;
    let mut receipt = json!({"name":name,"id":id,"region":plan.region,"size":plan.size["slug"],"price_hourly":plan.size["price_hourly"],"image":"ubuntu-24-04-x64"});
    crate::write_json(output.join("droplet.json"), &receipt)?;
    let deadline = Instant::now() + Duration::from_secs(600);
    let (public, private, vpc) = loop {
        ensure!(
            !cancel.cancelled() && Instant::now() < deadline,
            "paired droplet readiness timed out"
        );
        let d = api
            .request("GET", &format!("/droplets/{id}"), None)?
            .context("droplet disappeared")?;
        if d["droplet"]["status"] == "active" {
            break addresses(&d["droplet"])?;
        }
        pause(cancel)?;
    };
    if let Some(paired) = paired {
        ensure!(
            vpc == paired.vpc
                && private != paired.private
                && id != crate::number(&paired.receipt, "id")?,
            "peers must be distinct droplets in the same VPC"
        );
    }
    receipt["public_address"] = json!(public.to_string());
    receipt["private_address"] = json!(private.to_string());
    receipt["vpc_uuid"] = json!(vpc);
    crate::write_json(output.join("droplet.json"), &receipt)?;
    let known = state.join("known_hosts");
    crate::write(
        &known,
        format!(
            "{public} {}",
            fs::read_to_string(host.with_extension("pub"))?
        ),
    )?;
    let options = vec![
        "-i".into(),
        client.display().to_string(),
        "-o".into(),
        "BatchMode=yes".into(),
        "-o".into(),
        "StrictHostKeyChecking=yes".into(),
        "-o".into(),
        format!("UserKnownHostsFile={}", known.display()),
        "-o".into(),
        "ConnectTimeout=10".into(),
        "-o".into(),
        "ServerAliveInterval=15".into(),
        "-o".into(),
        "ServerAliveCountMax=3".into(),
        "-o".into(),
        "IdentitiesOnly=yes".into(),
    ];
    loop {
        ensure!(
            !cancel.cancelled() && Instant::now() < deadline,
            "paired SSH readiness timed out"
        );
        let mut args = options.clone();
        args.extend([format!("root@{public}"), "true".into()]);
        if commands.ready(&args, cancel) {
            break;
        }
        pause(cancel)?;
    }
    commands.ssh(
        &options,
        &public.to_string(),
        "cloud-init status --wait",
        240,
        cancel,
    )?;
    commands.ssh(&options, &public.to_string(), "DEBIAN_FRONTEND=noninteractive apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install --no-install-recommends -y valgrind util-linux time iproute2 iptables && mkdir -p /root/bench /root/results/remote", 300, cancel)?;
    Ok(Host {
        public,
        private,
        options,
        plan,
        vpc,
        receipt,
    })
}
fn upload(
    commands: &impl Commands,
    host: &Host,
    archive: &Path,
    cancel: &Cancellation,
) -> Result<()> {
    let mut args = host.options.clone();
    args.extend([
        archive.display().to_string(),
        format!("root@{}:/root/bundle.tar.gz", host.public),
    ]);
    commands.command("scp", &args, 300, cancel)?;
    commands.ssh(&host.options,&host.public.to_string(),"tar -xzf /root/bundle.tar.gz -C /root/bench && chmod +x /root/bench/driver /root/bench/capnp-reference",120,cancel)
}
fn retrieve(
    commands: &impl Commands,
    host: &Host,
    output: &Path,
    cancel: &Cancellation,
) -> Result<()> {
    fs::create_dir_all(output)?;
    commands.ssh(
        &host.options,
        &host.public.to_string(),
        "tar -czf /root/results/remote.tar.gz -C /root/results/remote .",
        120,
        cancel,
    )?;
    commands.download(
        &host.options,
        &host.public.to_string(),
        output,
        "remote.tar.gz",
        120,
        cancel,
    )?;
    unpack(&output.join("remote.tar.gz"), output)
}
fn unpack(archive: &Path, output: &Path) -> Result<()> {
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(fs::File::open(archive)?));
    let mut total = 0u64;
    for entry in archive.entries()? {
        let mut entry = entry?;
        if entry.header().entry_type().is_dir() {
            continue;
        }
        let path = entry.path()?.into_owned();
        let parts: Vec<_> = path
            .components()
            .filter(|c| *c != std::path::Component::CurDir)
            .collect();
        ensure!(
            entry.header().entry_type().is_file()
                && matches!(parts.as_slice(), [std::path::Component::Normal(_)]),
            "unexpected remote artifact path"
        );
        total = total
            .checked_add(entry.size())
            .context("remote artifact size overflow")?;
        ensure!(
            total <= 512 * 1024 * 1024,
            "remote artifacts exceed size limit"
        );
        let name = path.file_name().context("remote artifact missing name")?;
        ensure!(
            name != "remote.tar.gz",
            "remote archive cannot replace itself"
        );
        entry.unpack(output.join(name))?;
    }
    Ok(())
}
pub(super) fn cleanup_receipt(api: &impl Api, receipt: &Value, owner: &str) -> Result<()> {
    let names = if let Some(names) = receipt["names"].as_array() {
        names
            .iter()
            .map(|name| name.as_str().context("invalid cleanup name"))
            .collect::<Result<Vec<_>>>()?
    } else {
        vec![crate::string(receipt, "name")?]
    };
    ensure!(
        !names.is_empty() && names.len() <= 2,
        "invalid cleanup receipt"
    );
    ensure!(
        names
            .iter()
            .all(|name| name.starts_with(&format!("{owner}-"))),
        "foreign cleanup receipt"
    );
    let mut errors = Vec::new();
    for name in names {
        if let Err(error) = cleanup(api, name, owner) {
            errors.push(error.to_string());
        }
    }
    ensure!(errors.is_empty(), "{}", errors.join("; "));
    Ok(())
}
pub(super) fn run(
    api: &impl Api,
    bundle: &Path,
    output: &Path,
    state: &Path,
    owner: &str,
    cancel: &Cancellation,
    commands: &impl Commands,
) -> Result<()> {
    let manifest = crate::read_json(bundle.join("manifest.json"))?;
    for name in [
        "driver",
        "capnp-reference",
        "tcp",
        "native",
        "grpc",
        "websocket",
        "capnp_cpp",
        "hot_paths",
        "gungraun-runner",
    ] {
        ensure!(
            manifest["binaries"][name] == crate::file_hash(bundle.join(name))?,
            "invalid bundle binary: {name}"
        );
    }
    for ancestor in state.ancestors() {
        ensure!(!ancestor.is_symlink(), "state path contains symlink");
    }
    ensure!(
        !state.join("receipt.json").exists(),
        "state already has a receipt; use a fresh state directory"
    );
    fs::create_dir_all(state)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(state, fs::Permissions::from_mode(0o700))?;
    }
    let remote = output.join("remote");
    ensure!(
        !remote.exists(),
        "remote output already exists; preserve the previous evidence and use a fresh output"
    );
    fs::create_dir_all(&remote)?;
    let run = std::env::var("GITHUB_RUN_ID").unwrap_or_else(|_| std::process::id().to_string());
    let attempt = std::env::var("GITHUB_RUN_ATTEMPT").unwrap_or("1".into());
    ensure!(
        run.parse::<u64>().is_ok() && attempt.parse::<u64>().is_ok(),
        "invalid workflow identity"
    );
    let prefix = format!("{owner}-{}-{run}-{attempt}", chrono::Utc::now().timestamp());
    let receipt = json!({"names":[format!("{prefix}-server"),format!("{prefix}-client")]});
    // Record both intended names before the first mutating API request. Cleanup
    // can find either resource even when a create response is lost.
    crate::write_json(state.join("receipt.json"), &receipt)?;
    let mut hosts = Vec::new();
    let result = (|| -> Result<()> {
        for (index, role) in ["server", "client"].into_iter().enumerate() {
            println!("Provisioning workflow {role} (two hosts, at most $1.00/hour combined)");
            let host = provision(
                api,
                commands,
                (&state.join(role), &remote.join(role)),
                receipt["names"][index].as_str().unwrap(),
                owner,
                hosts.first(),
                cancel,
            )?;
            hosts.push(host);
        }
        let (server, client) = (&hosts[0], &hosts[1]);
        crate::write_json(
            remote.join("topology.json"),
            &json!({"topology":"two-droplet-vpc","server":server.receipt,"client":client.receipt}),
        )?;
        let archive = state.join("bundle.tar.gz");
        let mut tar = tar::Builder::new(flate2::write::GzEncoder::new(
            fs::File::create(&archive)?,
            flate2::Compression::default(),
        ));
        for entry in fs::read_dir(bundle)? {
            let entry = entry?;
            ensure!(entry.file_type()?.is_file(), "unexpected bundle entry");
            tar.append_path_with_name(entry.path(), entry.file_name())?;
        }
        tar.into_inner()?.finish()?;
        for host in &hosts {
            upload(commands, host, &archive, cancel)?;
        }
        // Bind only the VPC address; admit benchmark TCP only from this run's
        // client. Management SSH stays on the independently pinned public path.
        commands.ssh(&server.options,&server.public.to_string(),&format!("iptables -I INPUT -p tcp --dport 46001:46005 -j REJECT && iptables -I INPUT -s {}/32 -d {}/32 -p tcp --dport 46001:46005 -j ACCEPT",client.private,server.private),30,cancel)?;
        commands.ssh(&server.options,&server.public.to_string(),&format!("nohup timeout --kill-after=10s 35m /root/bench/driver workflow-host /root/bench {} /root/results/remote > /root/results/remote/runner.log 2>&1 < /dev/null &",server.private),30,cancel)?;
        let deadline = Instant::now() + Duration::from_secs(120);
        loop {
            ensure!(
                Instant::now() < deadline,
                "remote workflow servers failed to become ready"
            );
            let mut args = server.options.clone();
            args.extend([
                format!("root@{}", server.public),
                "test -f /root/results/remote/server-ready.json".into(),
            ]);
            if commands.ready(&args, cancel) {
                break;
            }
            pause(cancel)?;
        }
        commands.ssh(&client.options,&client.public.to_string(),&format!("bash -o pipefail -c 'timeout --kill-after=10s 25m /root/bench/driver workflow-client /root/bench {} /root/results/remote 2>&1 | tee /root/results/remote/runner.log'",server.private),1520,cancel)?;
        commands.ssh(&server.options,&server.public.to_string(),"touch /root/results/remote/stop && timeout 30s sh -c 'until test -f /root/results/remote/server-stopped; do sleep 1; done'",40,cancel)?;
        retrieve(commands, server, &remote.join("server"), cancel)?;
        retrieve(commands, client, &remote.join("client"), cancel)?;
        report::generate(&remote, bundle)?;
        // Preserve the existing instruction and loopback reports as explicitly
        // separate controls, on the already-provisioned client after remote work.
        commands.ssh(&client.options,&client.public.to_string(),"bash -o pipefail -c 'CAPNTPROTO_BENCH_PORTABLE=0 timeout --kill-after=10s 20m /root/bench/driver compare /root/bench /root/results 2>&1 | tee /root/results/runner.log'",1250,cancel)?;
        for file in [
            "trials.json",
            "environment.json",
            "instructions.jsonl",
            "runner.log",
        ] {
            commands.download(
                &client.options,
                &client.public.to_string(),
                output,
                file,
                120,
                cancel,
            )?;
        }
        Ok(())
    })();
    if result.is_err() {
        let salvage = Cancellation::default();
        for (host, role) in hosts.iter().zip(["server", "client"]) {
            let _ = commands.ssh(
                &host.options,
                &host.public.to_string(),
                "touch /root/results/remote/stop",
                15,
                &salvage,
            );
            if let Err(error) = retrieve(commands, host, &remote.join(role), &salvage) {
                eprintln!("Could not salvage {role} artifacts: {error}");
            }
        }
    }
    let cleaned = cleanup_receipt(api, &receipt, owner);
    if cleaned.is_ok() {
        for role in ["server", "client"] {
            for key in ["client", "client.pub", "host", "host.pub"] {
                crate::remove(state.join(role).join(key))?;
            }
        }
    }
    match (result, cleaned) {
        (Err(e), Err(clean)) => bail!("{e}; {clean}"),
        (Err(e), _) | (_, Err(e)) => Err(e),
        _ => Ok(()),
    }
}
#[cfg(test)]
mod tests;

pub(super) fn smoke(bundle: &Path, output: &Path) -> Result<()> {
    ensure!(!output.exists(), "smoke output must be fresh");
    fs::create_dir_all(output.join("server"))?;
    struct Child(std::process::Child);
    impl Drop for Child {
        fn drop(&mut self) {
            #[cfg(unix)]
            {
                let _ = nix::sys::signal::killpg(
                    nix::unistd::Pid::from_raw(self.0.id() as i32),
                    nix::sys::signal::Signal::SIGKILL,
                );
            }
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut command = Command::new(bundle.join("driver"));
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut server = Child(
        command
            .arg("workflow-host")
            .arg(bundle)
            .arg("127.0.0.1")
            .arg(output.join("server"))
            .arg("--smoke")
            .stdout(fs::File::create(output.join("server.log"))?)
            .stderr(fs::File::create(output.join("server.stderr.log"))?)
            .spawn()?,
    );
    let deadline = Instant::now() + Duration::from_secs(60);
    while !output.join("server/server-ready.json").exists() {
        ensure!(
            server.0.try_wait()?.is_none(),
            "workflow smoke server failed; see server.stderr.log"
        );
        ensure!(
            Instant::now() < deadline,
            "workflow smoke readiness timed out"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    crate::process::live(
        Command::new(bundle.join("driver"))
            .arg("workflow-client")
            .arg(bundle)
            .arg("127.0.0.1")
            .arg(output.join("client"))
            .arg("--smoke"),
        180,
        &Cancellation::default(),
    )?;
    // A self-to-self run could hide matching client/server mistakes. Exercise
    // the wire protocol against C++ in both directions, for both Rust drivers.
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("../../../benchmarks/rpc/workflows.json"))?;
    for case in cases {
        let name = crate::string(&case, "name")?;
        for (client, port, portable) in [
            ("driver", 46003, "0"),
            ("driver", 46003, "1"),
            ("capnp-reference", 46001, "0"),
            ("capnp-reference", 46002, "1"),
        ] {
            let log = output
                .join("interop")
                .join(format!("{name}-{client}-{portable}.log"));
            let result = crate::process::run(
                Command::new(bundle.join(client))
                    .args([
                        "workflow-measure",
                        &format!("127.0.0.1:{port}"),
                        name,
                        "2",
                        "5",
                    ])
                    .env("CAPNTPROTO_BENCH_PORTABLE", portable),
                30,
                Some(&log),
                &Cancellation::default(),
            )?;
            ensure!(
                result.code == 0,
                "workflow interoperability failed: {}",
                log.display()
            );
        }
    }
    crate::write(output.join("server/stop"), "stop\n")?;
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Some(status) = server.0.try_wait()? {
            ensure!(status.success(), "workflow smoke server failed");
            break;
        }
        ensure!(
            Instant::now() < deadline,
            "workflow smoke shutdown timed out"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    println!("All 58 supported workflow/implementation cells and 48 cross-language checks passed; these are not performance measurements.");
    Ok(())
}
