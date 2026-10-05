//! Ephemeral dedicated benchmark hosts. Never retry ambiguous resource creation.
use crate::process::Cancellation;
use anyhow::{bail, ensure, Context, Result};
use clap::{Parser, ValueEnum};
use serde_json::{json, Value};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    process::Command,
    time::{Duration, Instant},
};
const REGIONS: [&str; 11] = [
    "nyc3", "nyc1", "nyc2", "tor1", "sfo3", "ams3", "lon1", "fra1", "sgp1", "blr1", "syd1",
];
#[derive(Clone, ValueEnum)]
pub enum Action {
    Check,
    Run,
    Cleanup,
    Janitor,
}
#[derive(Parser)]
pub struct Args {
    pub action: Action,
    #[arg(long)]
    pub bundle: Option<PathBuf>,
    #[arg(long)]
    pub output: Option<PathBuf>,
    #[arg(long)]
    pub state: PathBuf,
}
#[derive(Debug)]
pub struct ProviderError {
    pub status: u16,
    pub code: String,
    pub message: String,
    pub method: String,
    pub path: String,
}
impl std::fmt::Display for ProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "DigitalOcean {} {}: HTTP {}: {} {}",
            self.method, self.path, self.status, self.code, self.message
        )
    }
}
impl std::error::Error for ProviderError {}
impl ProviderError {
    fn capacity(&self) -> bool {
        let m = self.message.to_lowercase();
        self.status == 422
            && (m.contains("insufficient capacity")
                || m.contains("at capacity")
                || (m.contains("size") && m.contains("not available") && m.contains("region")))
    }
}
fn regex(s: &str) -> regex::Regex {
    regex::Regex::new(s).expect("constant expression")
}
pub fn error_detail(body: &Value, token: &str, data: Option<&Value>) -> (String, String) {
    let code = body["id"]
        .as_str()
        .filter(|s| regex(r"^[a-zA-Z0-9_-]{1,80}$").is_match(s))
        .unwrap_or("")
        .to_owned();
    let mut message = body["message"].as_str().unwrap_or("").to_owned();
    if !token.is_empty() {
        message = message.replace(token, "[redacted]");
    }
    if let Some(user) = data
        .and_then(|d| d["user_data"].as_str())
        .filter(|s| !s.is_empty())
    {
        message = message.replace(user, "[cloud-init redacted]");
        if let Ok(config) =
            serde_json::from_str::<Value>(user.trim_start_matches("#cloud-config\n"))
        {
            if let Some(keys) = config["ssh_keys"].as_object() {
                for value in keys
                    .values()
                    .filter_map(Value::as_str)
                    .filter(|v| !v.is_empty())
                {
                    message = message.replace(value, "[key redacted]");
                }
            }
        }
    }
    message = regex(r"(?s)-----BEGIN [^-]*PRIVATE KEY-----.*?-----END [^-]*PRIVATE KEY-----")
        .replace_all(&message, "[private key redacted]")
        .into_owned();
    (
        code,
        message
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(512)
            .collect(),
    )
}
trait Api {
    fn request(&self, method: &str, path: &str, data: Option<&Value>) -> Result<Option<Value>>;
    fn pages(&self, path: &str, key: &str) -> Result<Vec<Value>> {
        let mut result = Vec::new();
        for page in 1..=100 {
            let sep = if path.contains('?') { '&' } else { '?' };
            let Some(value) =
                self.request("GET", &format!("{path}{sep}per_page=200&page={page}"), None)?
            else {
                return Ok(result);
            };
            result.extend(
                value[key]
                    .as_array()
                    .context("invalid provider page")?
                    .iter()
                    .cloned(),
            );
            if value["links"]["pages"]["next"]
                .as_str()
                .is_none_or(str::is_empty)
            {
                return Ok(result);
            }
        }
        bail!("DigitalOcean pagination exceeded limit")
    }
}
struct Http {
    token: String,
    client: reqwest::blocking::Client,
}
impl Http {
    fn new() -> Result<Self> {
        let token = std::env::var("DIGITALOCEAN_ACCESS_TOKEN")
            .context("DIGITALOCEAN_ACCESS_TOKEN is required")?;
        ensure!(!token.is_empty(), "DIGITALOCEAN_ACCESS_TOKEN is empty");
        Ok(Self {
            token,
            client: reqwest::blocking::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(30))
                .build()?,
        })
    }
}
impl Api for Http {
    fn request(&self, method: &str, path: &str, data: Option<&Value>) -> Result<Option<Value>> {
        let attempts = if method == "POST" { 1 } else { 4 };
        for attempt in 0..attempts {
            let mut request = self
                .client
                .request(
                    method.parse()?,
                    format!("https://api.digitalocean.com/v2{path}"),
                )
                .bearer_auth(&self.token);
            if let Some(data) = data {
                request = request.json(data);
            }
            match request.send() {
                Ok(response) => {
                    let status = response.status().as_u16();
                    if status == 404 && matches!(method, "GET" | "DELETE") {
                        return Ok(None);
                    }
                    if response.status().is_success() {
                        let bytes = response.bytes()?;
                        return Ok(if bytes.is_empty() {
                            None
                        } else {
                            Some(serde_json::from_slice(&bytes)?)
                        });
                    }
                    if ![429, 500, 502, 503, 504].contains(&status) || attempt + 1 == attempts {
                        use std::io::Read;
                        let mut bytes = Vec::new();
                        response.take(16 * 1024).read_to_end(&mut bytes)?;
                        let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
                        let (code, message) = error_detail(&body, &self.token, data);
                        return Err(ProviderError {
                            status,
                            code,
                            message,
                            method: method.into(),
                            path: path.into(),
                        }
                        .into());
                    }
                }
                Err(_) => {
                    if attempt + 1 == attempts {
                        bail!("DigitalOcean {method} {path}: connection failed");
                    }
                }
            }
            std::thread::sleep(Duration::from_secs(1 << attempt));
        }
        unreachable!()
    }
}
fn tag(repo: &str) -> String {
    // Retain the existing provider tag so migration cannot orphan hosts or SSH
    // keys created by the previous runner, including its hourly janitor scope.
    format!("reproto-bench-{}", &crate::hash(repo)[..16])
}
fn droplets(api: &impl Api, owner: &str) -> Result<Vec<Value>> {
    api.pages(&format!("/droplets?tag_name={owner}"), "droplets")
}
fn owned(d: &Value, name: &str, owner: &str) -> bool {
    d["name"] == name
        && d["tags"]
            .as_array()
            .is_some_and(|tags| tags.iter().any(|t| t == owner))
}
fn cleanup(api: &impl Api, name: &str, owner: &str) -> Result<()> {
    ensure!(
        name.starts_with(&format!("{owner}-")),
        "receipt does not belong to this repository"
    );
    let mut errors = Vec::new();
    let result = (|| -> Result<()> {
        for d in droplets(api, owner)? {
            if owned(&d, name, owner) {
                let result = (|| -> Result<()> {
                    let id = crate::number(&d, "id")?;
                    let path = format!("/droplets/{id}");
                    api.request("DELETE", &path, None)?;
                    let deadline = Instant::now() + Duration::from_secs(180);
                    loop {
                        if api.request("GET", &path, None)?.is_none() {
                            println!("Destroyed benchmark droplet {id}");
                            break;
                        }
                        ensure!(
                            Instant::now() < deadline,
                            "Droplet {id} deletion is not confirmed; check DigitalOcean"
                        );
                        std::thread::sleep(Duration::from_secs(5));
                    }
                    Ok(())
                })();
                if let Err(e) = result {
                    errors.push(e.to_string());
                }
            }
        }
        Ok(())
    })();
    if let Err(e) = result {
        errors.push(e.to_string());
    }
    let result = (|| -> Result<()> {
        for key in api.pages("/account/keys", "ssh_keys")? {
            if key["name"] == name {
                if let Err(e) = api.request(
                    "DELETE",
                    &format!("/account/keys/{}", crate::number(&key, "id")?),
                    None,
                ) {
                    errors.push(e.to_string());
                }
            }
        }
        Ok(())
    })();
    if let Err(e) = result {
        errors.push(e.to_string());
    }
    ensure!(errors.is_empty(), "Cleanup failed: {}", errors.join("; "));
    Ok(())
}
fn janitor(api: &impl Api, owner: &str) -> Result<()> {
    let now = chrono::Utc::now().timestamp();
    let prefix = format!("{owner}-");
    let mut names = BTreeSet::new();
    for d in droplets(api, owner)? {
        let name = crate::string(&d, "name")?;
        let created =
            chrono::DateTime::parse_from_rfc3339(crate::string(&d, "created_at")?)?.timestamp();
        if name.starts_with(&prefix) && owned(&d, name, owner) && now - created > 7200 {
            names.insert(name.to_owned());
        }
    }
    for key in api.pages("/account/keys", "ssh_keys")? {
        let name = crate::string(&key, "name")?;
        if let Some(rest) = name.strip_prefix(&prefix) {
            if rest
                .split('-')
                .next()
                .and_then(|s| s.parse::<i64>().ok())
                .is_some_and(|t| now - t > 7200)
            {
                names.insert(name.to_owned());
            }
        }
    }
    for name in names {
        cleanup(api, &name, owner)?;
    }
    Ok(())
}
#[derive(Clone, Debug)]
struct Plan {
    size: Value,
    region: String,
}
fn select(
    sizes: Vec<Value>,
    size: &str,
    region: &str,
    excluded: &BTreeSet<(String, String)>,
) -> Result<Plan> {
    let size_re = regex(r"^c-[0-9]+(?:-intel)?$");
    let region_re = regex(r"^[a-z]{3}[0-9]+$");
    ensure!(
        size == "auto" || size_re.is_match(size),
        "Use a dedicated CPU-Optimized c-N plan (shared CPUs are rejected)"
    );
    ensure!(
        region == "auto" || region_re.is_match(region),
        "Invalid DigitalOcean region slug"
    );
    let mut candidates = Vec::new();
    let mut alternatives = BTreeSet::new();
    for s in sizes {
        let Some(slug) = s["slug"].as_str().filter(|s| size_re.is_match(s)) else {
            continue;
        };
        let price = s["price_hourly"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| s["price_hourly"].to_string())
            .parse::<rust_decimal::Decimal>();
        let valid = price.as_ref().is_ok_and(|p| {
            *p > rust_decimal::Decimal::ZERO && *p <= rust_decimal::Decimal::new(50, 2)
        });
        if !valid {
            ensure!(
                slug != size,
                "Dedicated plan exceeds the $0.50/hour rate ceiling or has an invalid rate"
            );
            continue;
        }
        if s["available"] != true {
            continue;
        }
        for r in s["regions"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .filter(|r| region_re.is_match(r))
        {
            alternatives.insert(format!("{slug}@{r}"));
            if (region == "auto" || region == r)
                && (if size == "auto" {
                    ["c-4", "c-4-intel"].contains(&slug) && s["vcpus"] == 4
                } else {
                    slug == size
                })
                && !excluded.contains(&(slug.to_owned(), r.to_owned()))
            {
                candidates.push((
                    slug != "c-4",
                    REGIONS
                        .iter()
                        .position(|x| *x == r)
                        .unwrap_or(REGIONS.len()),
                    price.clone().unwrap(),
                    slug.to_owned(),
                    r.to_owned(),
                    s.clone(),
                ));
            }
        }
    }
    candidates.sort_by(|a, b| (&a.0, &a.1, &a.2, &a.3, &a.4).cmp(&(&b.0, &b.1, &b.2, &b.3, &b.4)));
    let Some((_, _, _, _, region, size_value)) = candidates.into_iter().next() else {
        bail!("No available dedicated plan for size={size}, region={region} under $0.50/hour. Use region=auto and size=auto for four dedicated vCPUs, or select an available pair. Available dedicated pairs: {}",alternatives.into_iter().take(40).collect::<Vec<_>>().join(", "));
    };
    Ok(Plan {
        size: size_value,
        region,
    })
}
fn select_plan(api: &impl Api, excluded: &BTreeSet<(String, String)>) -> Result<Plan> {
    select(
        api.pages("/sizes", "sizes")?,
        &std::env::var("DO_SIZE").unwrap_or("auto".into()),
        &std::env::var("DO_REGION").unwrap_or("auto".into()),
        excluded,
    )
}
fn preflight(api: &impl Api, output: Option<&Path>) -> Result<Plan> {
    if let Some(out) = output {
        crate::remove(out.join("capacity.json"))?;
    }
    let result = select_plan(api, &BTreeSet::new());
    if let Err(e) = &result {
        if let Some(out) = output {
            crate::write_json(
                out.join("capacity.json"),
                &json!({"status":"unavailable","reason":e.to_string()}),
            )?;
        }
    }
    let plan = result?;
    let record = json!({"size":plan.size["slug"],"region":plan.region,"price_hourly":plan.size["price_hourly"],"vcpus":plan.size["vcpus"],"image":"ubuntu-24-04-x64"});
    println!("Eligible catalog plan (capacity is confirmed at creation): {record}");
    if let Some(out) = output {
        crate::write_json(out.join("capacity.json"), &record)?;
    }
    Ok(plan)
}
fn create_host(
    api: &impl Api,
    mut payload: Value,
    mut plan: Plan,
    output: &Path,
    owner: &str,
    cancel: &Cancellation,
    select_next: impl Fn(&BTreeSet<(String, String)>) -> Result<Plan>,
) -> Result<(Value, Plan)> {
    let mut attempts = Vec::new();
    let mut excluded = BTreeSet::new();
    let path = output.join("provisioning.json");
    for _ in 0..24 {
        ensure!(!cancel.cancelled(), "operation interrupted");
        payload["size"] = plan.size["slug"].clone();
        payload["region"] = json!(plan.region);
        attempts.push(json!({"size":plan.size["slug"],"region":plan.region,"price_hourly":plan.size["price_hourly"],"status":"requesting"}));
        crate::write_json(&path, &json!(attempts))?;
        println!(
            "Creating dedicated benchmark host: {}@{}",
            plan.size["slug"], plan.region
        );
        match api.request("POST", "/droplets", Some(&payload)) {
            Ok(Some(value)) => {
                let host = value["droplet"].clone();
                let id = crate::number(&host, "id")?;
                let attempt = attempts.last_mut().unwrap();
                attempt["status"] = json!("created");
                attempt["id"] = json!(id);
                crate::write_json(&path, &json!(attempts))?;
                return Ok((host, plan));
            }
            result => {
                let error = result
                    .err()
                    .unwrap_or_else(|| anyhow::anyhow!("empty creation response"));
                let provider = error.downcast_ref::<ProviderError>();
                let attempt = attempts.last_mut().unwrap();
                attempt["status"] = json!(if provider.is_some() {
                    "rejected"
                } else {
                    "unknown"
                });
                attempt["error"] = json!(if provider.is_some() {
                    error.to_string()
                } else {
                    "ambiguous provider response".into()
                });
                crate::write_json(&path, &json!(attempts))?;
                if !provider.is_some_and(ProviderError::capacity) {
                    return Err(error);
                }
                println!("{error}");
                ensure!(
                    !droplets(api, owner)?.iter().any(|d| owned(
                        d,
                        payload["name"].as_str().unwrap_or(""),
                        owner
                    )),
                    "Rejected creation has a matching Droplet; refusing another POST"
                );
                excluded.insert((
                    crate::string(&plan.size, "slug")?.to_owned(),
                    plan.region.clone(),
                ));
                ensure!(
                    attempts.len() < 24,
                    "Dedicated capacity fallback limit reached; see provisioning.json"
                );
                plan = select_next(&excluded)?;
            }
        }
    }
    unreachable!()
}
fn pause(cancel: &Cancellation) -> Result<()> {
    for _ in 0..50 {
        ensure!(!cancel.cancelled(), "operation interrupted");
        std::thread::sleep(Duration::from_millis(100));
    }
    Ok(())
}
trait Commands {
    fn command(
        &self,
        program: &str,
        args: &[String],
        seconds: u64,
        cancel: &Cancellation,
    ) -> Result<()>;
    fn ready(&self, args: &[String], cancel: &Cancellation) -> bool;
    fn ssh(
        &self,
        options: &[String],
        address: &str,
        remote: &str,
        seconds: u64,
        cancel: &Cancellation,
    ) -> Result<()> {
        let mut args = options.to_vec();
        args.extend([format!("root@{address}"), remote.to_owned()]);
        self.command("ssh", &args, seconds, cancel)
    }
    fn download(
        &self,
        options: &[String],
        address: &str,
        output: &Path,
        file: &str,
        seconds: u64,
        cancel: &Cancellation,
    ) -> Result<()> {
        let mut args = options.to_vec();
        args.extend([
            format!("root@{address}:/root/results/{file}"),
            output.join(file).display().to_string(),
        ]);
        self.command("scp", &args, seconds, cancel)
    }
}
struct SystemCommands;
impl Commands for SystemCommands {
    fn command(
        &self,
        program: &str,
        args: &[String],
        seconds: u64,
        cancel: &Cancellation,
    ) -> Result<()> {
        crate::process::live(Command::new(program).args(args), seconds, cancel)
    }
    fn ready(&self, args: &[String], cancel: &Cancellation) -> bool {
        crate::process::run(Command::new("ssh").args(args), 20, None, cancel)
            .is_ok_and(|r| r.code == 0)
    }
}
fn benchmark(
    api: &impl Api,
    bundle: &Path,
    output: &Path,
    state: &Path,
    owner: &str,
    cancel: &Cancellation,
    commands: &impl Commands,
) -> Result<()> {
    std::fs::create_dir_all(output)?;
    for file in [
        "trials.json",
        "environment.json",
        "instructions.jsonl",
        "runner.log",
        "capacity.json",
        "provisioning.json",
        "droplet.json",
    ] {
        crate::remove(output.join(file))?;
    }
    let plan = preflight(api, Some(output))?;
    let run = std::env::var("GITHUB_RUN_ID")?;
    let attempt = std::env::var("GITHUB_RUN_ATTEMPT").unwrap_or("1".into());
    ensure!(
        run.parse::<u64>().is_ok() && attempt.parse::<u64>().is_ok(),
        "invalid workflow identity"
    );
    let name = format!("{owner}-{}-{run}-{attempt}", chrono::Utc::now().timestamp());
    for ancestor in std::path::absolute(state)?.ancestors() {
        ensure!(
            !ancestor.is_symlink(),
            "state directory must not contain symlinks"
        );
    }
    std::fs::create_dir_all(state)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(state, std::fs::Permissions::from_mode(0o700))?;
    }
    ensure!(
        !state.join("client").exists() && !state.join("host").exists(),
        "state contains existing keys; clean up the previous run first"
    );
    crate::write_json(state.join("receipt.json"), &json!({"name":name}))?;
    let client = state.join("client");
    let host = state.join("host");
    let result = (|| -> Result<()> {
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
        let key=api.request("POST","/account/keys",Some(&json!({"name":name,"public_key":std::fs::read_to_string(client.with_extension("pub"))?})))?.context("missing SSH key response")?;
        ensure!(!cancel.cancelled(), "operation interrupted");
        let config = json!({"ssh_pwauth":false,"disable_root":false,"ssh_keys":{"ed25519_private":std::fs::read_to_string(&host)?,"ed25519_public":std::fs::read_to_string(host.with_extension("pub"))?}});
        let payload = json!({"name":name,"image":"ubuntu-24-04-x64","ssh_keys":[crate::number(&key["ssh_key"],"id")?],"tags":[owner],"backups":false,"monitoring":false,"with_droplet_agent":false,"user_data":format!("#cloud-config\n{config}")});
        let (created, plan) = create_host(api, payload, plan, output, owner, cancel, |excluded| {
            select_plan(api, excluded)
        })?;
        let id = crate::number(&created, "id")?;
        let receipt = json!({"name":name,"id":id,"region":plan.region,"size":plan.size["slug"],"price_hourly":plan.size["price_hourly"],"image":"ubuntu-24-04-x64"});
        crate::write_json(state.join("receipt.json"), &receipt)?;
        crate::write_json(output.join("droplet.json"), &receipt)?;
        println!("Benchmark droplet: {receipt}");
        let deadline = Instant::now() + Duration::from_secs(600);
        let address = loop {
            ensure!(
                Instant::now() < deadline,
                "Droplet did not become active within ten minutes"
            );
            let current = api
                .request("GET", &format!("/droplets/{id}"), None)?
                .context("droplet disappeared")?;
            let d = &current["droplet"];
            let address = d["networks"]["v4"]
                .as_array()
                .into_iter()
                .flatten()
                .find(|n| d["status"] == "active" && n["type"] == "public")
                .and_then(|n| n["ip_address"].as_str());
            if let Some(address) = address {
                break address.parse::<std::net::Ipv4Addr>()?.to_string();
            }
            pause(cancel)?;
        };
        let known = state.join("known_hosts");
        crate::write(
            &known,
            format!(
                "{address} {}",
                std::fs::read_to_string(host.with_extension("pub"))?
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
            ensure!(Instant::now() < deadline, "SSH readiness timed out");
            let mut args = options.clone();
            args.extend([format!("root@{address}"), "true".into()]);
            if commands.ready(&args, cancel) {
                break;
            }
            pause(cancel)?;
        }
        println!("SSH ready; waiting for cloud-init");
        commands.ssh(&options, &address, "cloud-init status --wait", 240, cancel)?;
        println!("Installing runtime tools (no compilation on the droplet)");
        commands.ssh(&options,&address,"DEBIAN_FRONTEND=noninteractive apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install --no-install-recommends -y valgrind util-linux",300,cancel)?;
        commands.ssh(
            &options,
            &address,
            "mkdir -p /root/bench /root/results",
            120,
            cancel,
        )?;
        let archive = state.join("bundle.tar.gz");
        let mut tar = tar::Builder::new(flate2::write::GzEncoder::new(
            std::fs::File::create(&archive)?,
            flate2::Compression::default(),
        ));
        let mut entries = std::fs::read_dir(bundle)?.collect::<std::io::Result<Vec<_>>>()?;
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            ensure!(entry.file_type()?.is_file(), "Unexpected bundle entry");
            tar.append_path_with_name(entry.path(), entry.file_name())?;
        }
        tar.into_inner()?.finish()?;
        println!(
            "Uploading precompiled bundle: {} bytes",
            std::fs::metadata(&archive)?.len()
        );
        let mut args = options.clone();
        args.extend([
            archive.display().to_string(),
            format!("root@{address}:/root/bundle.tar.gz"),
        ]);
        commands.command("scp", &args, 300, cancel)?;
        println!("Starting measurements; live trial progress follows (20-minute overall limit)");
        let remote="bash -o pipefail -c 'tar -xzf /root/bundle.tar.gz -C /root/bench && chmod +x /root/bench/native /root/bench/grpc /root/bench/websocket /root/bench/capnp_cpp /root/bench/capnp-reference /root/bench/driver /root/bench/hot_paths /root/bench/gungraun-runner && timeout --kill-after=10s 20m /root/bench/driver compare /root/bench /root/results 2>&1 | tee /root/results/runner.log'";
        if let Err(error) = commands.ssh(&options, &address, remote, 1250, cancel) {
            let salvage = Cancellation::default();
            for file in [
                "runner.log",
                "trials.json",
                "environment.json",
                "instructions.jsonl",
            ] {
                if let Err(e) = commands.download(&options, &address, output, file, 30, &salvage) {
                    eprintln!("Could not retrieve {file}: {e}");
                }
            }
            return Err(error);
        }
        println!("Measurements completed; downloading validated-input artifacts");
        for file in [
            "trials.json",
            "environment.json",
            "instructions.jsonl",
            "runner.log",
        ] {
            commands.download(&options, &address, output, file, 120, cancel)?;
        }
        Ok(())
    })();
    // Cleanup ignores cancellation so SIGTERM cannot abandon billable resources.
    let cleaned = cleanup(api, &name, owner);
    if cleaned.is_ok() {
        for key in [&client, &host] {
            crate::remove(key)?;
            crate::remove(key.with_extension("pub"))?;
        }
    }
    match (result, cleaned) {
        (Err(e), Err(clean)) => bail!("{e}; {clean}"),
        (Err(e), _) | (_, Err(e)) => Err(e),
        _ => Ok(()),
    }
}
pub fn run(args: Args) -> Result<()> {
    let api = Http::new()?;
    let owner = tag(&std::env::var("GITHUB_REPOSITORY")?);
    let cancel = Cancellation::install()?;
    match args.action {
        Action::Check => {
            preflight(&api, args.output.as_deref())?;
            Ok(())
        }
        Action::Cleanup => {
            let path = args.state.join("receipt.json");
            if path.exists() {
                cleanup(
                    &api,
                    crate::string(&crate::read_json(&path)?, "name")?,
                    &owner,
                )?;
            }
            Ok(())
        }
        Action::Janitor => janitor(&api, &owner),
        Action::Run => benchmark(
            &api,
            &args
                .bundle
                .context("run requires --bundle")?
                .canonicalize()?,
            &std::path::absolute(args.output.context("run requires --output")?)?,
            &std::path::absolute(args.state)?,
            &owner,
            &cancel,
            &SystemCommands,
        ),
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn size(slug: &str, price: Value) -> Value {
        json!({"slug":slug,"price_hourly":price,"available":true,"regions":["nyc3","tor1"],"vcpus":4})
    }
    #[test]
    fn bounded_capacity_and_explicit_choices() {
        let sizes = vec![
            size("s-4vcpu", json!(0.01)),
            size("c-4", json!(0.1)),
            size("c-4-intel", json!(0.11)),
        ];
        let plan = select(sizes.clone(), "auto", "auto", &BTreeSet::new()).unwrap();
        assert_eq!(plan.size["slug"], "c-4");
        assert_eq!(plan.region, "nyc3");
        let excluded = BTreeSet::from([("c-4".into(), "nyc3".into())]);
        assert_eq!(
            select(sizes.clone(), "auto", "auto", &excluded)
                .unwrap()
                .region,
            "tor1"
        );
        assert!(select(sizes, "c-4", "nyc3", &excluded).is_err());
        assert!(select(
            vec![size("c-4", json!("0.5000000000000000001"))],
            "c-4",
            "auto",
            &BTreeSet::new()
        )
        .is_err());
    }
    #[test]
    fn redaction_covers_partial_provider_echoes() {
        let key = "-----BEGIN OPENSSH PRIVATE KEY-----\nSECRET\n-----END OPENSSH PRIVATE KEY-----";
        let config = format!(
            "#cloud-config\n{}",
            json!({"ssh_keys":{"ed25519_private":key,"ed25519_public":"PUBLIC"}})
        );
        let (code, msg) = error_detail(
            &json!({"id":"bad_request","message":format!("TOKEN {key} PUBLIC {config}")}),
            "TOKEN",
            Some(&json!({"user_data":config})),
        );
        assert_eq!(code, "bad_request");
        for secret in ["TOKEN", "SECRET", "PUBLIC", "cloud-config"] {
            assert!(!msg.contains(secret));
        }
    }
    #[test]
    fn only_capacity_is_retryable() {
        for (status, message, expected) in [
            (422, "size is not available in this region", true),
            (422, "Insufficient capacity", true),
            (422, "at capacity", true),
            (422, "quota exceeded", false),
            (429, "at capacity", false),
            (500, "insufficient capacity", false),
        ] {
            assert_eq!(
                ProviderError {
                    status,
                    code: "".into(),
                    message: message.into(),
                    method: "POST".into(),
                    path: "/droplets".into()
                }
                .capacity(),
                expected
            );
        }
    }
}

#[cfg(test)]
mod lifecycle_tests;
