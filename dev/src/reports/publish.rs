//! Privileged publication accepts only bounded data from trusted producer runs.
use super::{data, render};
use anyhow::{bail, ensure, Context, Result};
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use sha1::Digest;
use std::{
    collections::BTreeMap,
    io::{Cursor, Read},
    path::Path,
    time::Duration,
};
const MAX_ARTIFACT: u64 = 16 * 1024 * 1024;
const BRANCH: &str = "reports";
fn kind(path: &str) -> Option<&'static str> {
    match path {
        ".github/workflows/ci.yml" => Some("ci"),
        ".github/workflows/full-quality.yml" | ".github/workflows/verification-coverage.yml" => {
            Some("full")
        }
        ".github/workflows/benchmarks.yml" | ".github/workflows/performance.yml" => {
            Some("benchmark")
        }
        ".github/workflows/verification-tests.yml" => Some("cargo"),
        ".github/workflows/verification-models.yml" => Some("models"),
        ".github/workflows/verification-fuzz.yml" => Some("fuzz"),
        _ => None,
    }
}
#[derive(Debug)]
struct HttpError(u16);
impl std::fmt::Display for HttpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "GitHub API HTTP {}", self.0)
    }
}
impl std::error::Error for HttpError {}
trait Api {
    fn request(&self, path: &str, body: Option<&Value>, method: &str) -> Result<Value>;
    fn download(&self, path: &str) -> Result<Vec<u8>>;
    fn content(&self, path: &str, reference: &str) -> Result<Option<(Vec<u8>, String)>> {
        let result = self.request(
            &format!("/contents/{path}?ref={}", encode(reference)),
            None,
            "GET",
        );
        let item = match result {
            Err(e) if e.downcast_ref::<HttpError>().is_some_and(|e| e.0 == 404) => return Ok(None),
            r => r?,
        };
        ensure!(
            item["type"] == "file" && item["encoding"] == "base64",
            "unexpected repository file representation"
        );
        let content = crate::string(&item, "content")?
            .split_whitespace()
            .collect::<String>();
        Ok(Some((
            STANDARD.decode(content)?,
            crate::string(&item, "sha")?.into(),
        )))
    }
}
fn encode(s: &str) -> String {
    percent_encoding::utf8_percent_encode(s, percent_encoding::NON_ALPHANUMERIC).to_string()
}
struct GitHub {
    base: String,
    token: String,
    client: reqwest::blocking::Client,
}
impl GitHub {
    fn new(repository: &str, token: String) -> Result<Self> {
        ensure!(
            data::matches(r"^[\w.-]+/[\w.-]+$", repository) && !token.is_empty(),
            "repository identity and GITHUB_TOKEN are required"
        );
        Ok(Self {
            base: format!("https://api.github.com/repos/{repository}"),
            token,
            client: reqwest::blocking::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(60))
                .user_agent("capntproto-dev-reporting")
                .build()?,
        })
    }
    fn send(
        &self,
        path: &str,
        body: Option<&Value>,
        method: &str,
    ) -> Result<reqwest::blocking::Response> {
        ensure!(path.is_empty() || path.starts_with('/'), "invalid API path");
        let mut request = self
            .client
            .request(method.parse()?, format!("{}{path}", self.base))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .bearer_auth(&self.token);
        if let Some(body) = body {
            request = request.json(body);
        }
        Ok(request.send()?)
    }
}
fn read_bounded(response: reqwest::blocking::Response) -> Result<Vec<u8>> {
    ensure!(
        response.content_length().is_none_or(|n| n <= MAX_ARTIFACT),
        "publication response too large"
    );
    let mut bytes = Vec::new();
    response.take(MAX_ARTIFACT + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= MAX_ARTIFACT,
        "publication response too large"
    );
    Ok(bytes)
}
impl Api for GitHub {
    fn request(&self, path: &str, body: Option<&Value>, method: &str) -> Result<Value> {
        let response = self.send(path, body, method)?;
        if !response.status().is_success() {
            return Err(HttpError(response.status().as_u16()).into());
        }
        let bytes = read_bounded(response)?;
        Ok(if bytes.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&bytes)?
        })
    }
    fn download(&self, path: &str) -> Result<Vec<u8>> {
        let mut response = self.send(path, None, "GET")?;
        for _ in 0..5 {
            if response.status().is_redirection() {
                let url = reqwest::Url::parse(
                    response
                        .headers()
                        .get(reqwest::header::LOCATION)
                        .context("missing artifact redirect")?
                        .to_str()?,
                )?;
                ensure!(
                    url.scheme() == "https"
                        && url.username().is_empty()
                        && url.password().is_none(),
                    "unsafe artifact redirect"
                ); // No Authorization header is ever attached to storage requests.
                response = self.client.get(url).send()?;
            } else {
                if !response.status().is_success() {
                    return Err(HttpError(response.status().as_u16()).into());
                }
                return read_bounded(response);
            }
        }
        bail!("artifact redirect limit exceeded")
    }
}
fn trusted(run: &Value, repository: &str, branch: &str) -> bool {
    run["status"] == "completed"
        && kind(run["path"].as_str().unwrap_or("")).is_some()
        && ["push", "schedule", "workflow_dispatch"].contains(&run["event"].as_str().unwrap_or(""))
        && run["head_branch"] == branch
        && run["head_repository"]["full_name"] == repository
}
fn archive_data(content: Vec<u8>) -> Result<Value> {
    ensure!(
        content.len() as u64 <= MAX_ARTIFACT,
        "publication artifact too large"
    );
    let mut archive = zip::ZipArchive::new(Cursor::new(content))?;
    ensure!(
        archive.len() == 1,
        "unexpected publication archive contents"
    );
    let mut entry = archive.by_index(0)?;
    ensure!(
        entry.name() == "publication.json"
            && entry.size() <= MAX_ARTIFACT
            && entry.is_file()
            && entry.unix_mode().is_none_or(|m| m & 0o170000 != 0o120000),
        "unexpected publication archive contents"
    );
    let mut bytes = Vec::new();
    entry
        .by_ref()
        .take(MAX_ARTIFACT + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= MAX_ARTIFACT,
        "publication artifact too large"
    );
    let value = serde_json::from_slice(&bytes)?;
    data::validate_publication(&value)?;
    Ok(value)
}
fn artifact_data(api: &impl Api, run: &Value, lane: &str) -> Result<Value> {
    let response = api.request(
        &format!(
            "/actions/runs/{}/artifacts?per_page=100",
            crate::number(run, "id")?
        ),
        None,
        "GET",
    )?;
    let artifacts = response["artifacts"]
        .as_array()
        .context("missing artifacts")?;
    let name = format!("readme-data-{lane}-{}", crate::number(run, "run_attempt")?);
    let matches: Vec<_> = artifacts
        .iter()
        // Accept historical standalone artifacts, never the shared legacy name
        // for CI: each lane must own a different immutable artifact.
        .filter(|a| {
            (a["name"] == name
                || (kind(run["path"].as_str().unwrap_or("")) != Some("ci")
                    && a["name"] == "readme-data"))
                && a["expired"] == false
        })
        .collect();
    if matches.is_empty() {
        return Ok(data::unavailable(
            lane,
            "This workflow attempt produced no public measurement artifact.",
        ));
    }
    ensure!(
        matches.len() == 1 && crate::number(matches[0], "size_in_bytes")? <= MAX_ARTIFACT,
        "ambiguous or oversized publication artifact"
    );
    let mut data = archive_data(api.download(&format!(
        "/actions/artifacts/{}/zip",
        crate::number(matches[0], "id")?
    ))?)?;
    let expected = json!({"repository":run["head_repository"]["full_name"],"run_id":run["id"],"attempt":run["run_attempt"],"commit":run["head_sha"]});
    if data["origin"] != expected {
        return Ok(data::unavailable(
            lane,
            "No measurement artifact matches this workflow attempt and commit.",
        ));
    }
    ensure!(data["kind"] == lane, "publication workflow/lane mismatch");
    if run["conclusion"] != "success" && data["status"] == "passed" {
        data["status"] = json!("failed");
        data["charts"] = json!([]);
        data["note"] = json!("Workflow failed after measurement; see the CI run.");
    }
    Ok(data)
}
fn blob_sha(data: &[u8]) -> String {
    let mut hash = sha1::Sha1::new();
    hash.update(format!("blob {}\0", data.len()));
    hash.update(data);
    hex::encode(hash.finalize())
}
fn execute(api: &impl Api, event: &Value, repository: &str) -> Result<()> {
    let id = event["workflow_run"]["id"]
        .as_u64()
        .or_else(|| {
            event["inputs"]["run_id"]
                .as_str()
                .and_then(|s| s.parse().ok())
        })
        .context("a completed producer workflow run ID is required")?;
    let branch = crate::string(&api.request("", None, "GET")?, "default_branch")?.to_owned();
    ensure!(
        branch != BRANCH,
        "report branch must not be the source default branch"
    );
    let run = api.request(&format!("/actions/runs/{id}"), None, "GET")?;
    ensure!(
        trusted(&run, repository, &branch),
        "only trusted producer runs from this repository default branch can publish"
    );
    let lane = kind(crate::string(&run, "path")?).unwrap();
    let lanes: &[&str] = if lane == "ci" {
        &["cargo", "models", "fuzz", "benchmark"]
    } else {
        &[lane]
    };
    let mut records = Vec::new();
    for lane in lanes {
        let data = artifact_data(api, &run, lane)?;
        let record = json!({"run_id":run["id"],"attempt":run["run_attempt"],"commit":run["head_sha"],"date":run["created_at"],"url":run["html_url"],"conclusion":run["conclusion"],"data":data});
        render::validate_record(&record)?;
        records.push(record);
    }
    for attempt in 0..3 {
        let source_head = crate::string(
            &api.request(&format!("/git/ref/heads/{}", encode(&branch)), None, "GET")?["object"],
            "sha",
        )?
        .to_owned();
        let comparison = api.request(
            &format!(
                "/compare/{}...{source_head}",
                crate::string(&run, "head_sha")?
            ),
            None,
            "GET",
        )?;
        ensure!(
            comparison["merge_base_commit"]["sha"] == run["head_sha"],
            "measured commit is not an ancestor of the current default branch"
        );
        let head = match api.request(&format!("/git/ref/heads/{BRANCH}"), None, "GET") {
            Ok(v) => Some(crate::string(&v["object"], "sha")?.to_owned()),
            Err(e) if e.downcast_ref::<HttpError>().is_some_and(|e| e.0 == 404) => None,
            Err(e) => return Err(e),
        };
        let old = if let Some(head) = &head {
            api.content("docs/reports/history.json", head)?
        } else {
            api.content("quality/reporting/history-seed.json", &source_head)?
        };
        ensure!(
            head.is_none() || old.is_some(),
            "existing reports branch has no history; refusing to replace unrelated content"
        );
        let mut history = match old {
            Some((bytes, _)) => serde_json::from_slice(&bytes)?,
            None => render::empty_history(),
        };
        for record in &records {
            history = render::merge(&history, record)?;
        }
        let template = api
            .content("docs/reports.template.md", &source_head)?
            .context("Report template is missing from the default branch")?
            .0;
        let mut existing = BTreeMap::new();
        if let Some(head) = &head {
            if let Some((_, sha)) = api.content("README.md", head)? {
                existing.insert("README.md".to_owned(), sha);
            }
            let listing = api.request(&format!("/contents/docs/reports?ref={head}"), None, "GET");
            match listing {
                Ok(v) => {
                    for item in v.as_array().context("invalid report inventory")? {
                        if item["type"] == "file" {
                            existing.insert(
                                crate::string(item, "path")?.to_owned(),
                                crate::string(item, "sha")?.to_owned(),
                            );
                        }
                    }
                }
                Err(e) if e.downcast_ref::<HttpError>().is_some_and(|e| e.0 == 404) => {}
                Err(e) => return Err(e),
            }
        }
        let dir = tempfile::tempdir()?;
        let files = render::render(std::str::from_utf8(&template)?, &history, dir.path())?;
        let mut generated = BTreeMap::new();
        for file in files {
            generated.insert(
                file.strip_prefix(dir.path())?
                    .to_string_lossy()
                    .replace('\\', "/"),
                std::fs::read(file)?,
            );
        }
        let owned = render::owned();
        ensure!(
            generated.keys().all(|p| owned.contains(p)),
            "renderer attempted to publish a non-report path"
        );
        let mut entries = Vec::new();
        for (path, content) in &generated {
            if existing.get(path) != Some(&blob_sha(content)) {
                let blob = api.request(
                    "/git/blobs",
                    Some(&json!({"content":STANDARD.encode(content),"encoding":"base64"})),
                    "POST",
                )?;
                entries.push(json!({"path":path,"mode":"100644","type":"blob","sha":blob["sha"]}));
            }
        }
        for path in existing
            .keys()
            .filter(|p| owned.contains(*p) && !generated.contains_key(*p))
        {
            entries.push(json!({"path":path,"mode":"100644","type":"blob","sha":null}));
        }
        if entries.is_empty() {
            println!("README reports are already current.");
            return Ok(());
        }
        let mut tree_request = json!({"tree":entries});
        if let Some(head) = &head {
            tree_request["base_tree"] =
                api.request(&format!("/git/commits/{head}"), None, "GET")?["tree"]["sha"].clone();
        }
        let tree = api.request("/git/trees", Some(&tree_request), "POST")?["sha"].clone();
        let commit=api.request("/git/commits",Some(&json!({"message":format!("docs: refresh Capntproto reports (run {id})"),"tree":tree,"parents":head.iter().collect::<Vec<_>>()})),"POST")?["sha"].clone();
        let updated = if head.is_some() {
            api.request(
                &format!("/git/refs/heads/{BRANCH}"),
                Some(&json!({"sha":commit,"force":false})),
                "PATCH",
            )
        } else {
            api.request(
                "/git/refs",
                Some(&json!({"ref":format!("refs/heads/{BRANCH}"),"sha":commit})),
                "POST",
            )
        };
        match updated {
            Ok(_) => {
                println!("Published reports on {BRANCH} at {commit}.");
                return Ok(());
            }
            Err(e)
                if attempt < 2
                    && e.downcast_ref::<HttpError>()
                        .is_some_and(|e| [409, 422].contains(&e.0)) =>
            {
                std::thread::sleep(Duration::from_secs(1))
            }
            Err(e) => return Err(e),
        }
    }
    bail!("could not publish reports without overwriting concurrent work")
}
pub fn publish(event: &Path) -> Result<()> {
    let repository = std::env::var("GITHUB_REPOSITORY")?;
    let api = GitHub::new(&repository, std::env::var("GITHUB_TOKEN")?)?;
    execute(&api, &crate::read_json(event)?, &repository)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn untrusted_runs_cannot_publish() {
        let run = json!({"status":"completed","path":".github/workflows/verification-tests.yml","event":"push","head_branch":"main","head_repository":{"full_name":"owner/repo"}});
        assert!(trusted(&run, "owner/repo", "main"));
        for (key, value) in [
            ("event", json!("pull_request")),
            ("head_branch", json!("reports")),
            ("path", json!(".github/workflows/untrusted.yml")),
            ("status", json!("in_progress")),
            ("head_repository", json!({"full_name":"fork/repo"})),
        ] {
            let mut changed = run.clone();
            changed[key] = value;
            assert!(!trusted(&changed, "owner/repo", "main"));
        }
    }
    #[test]
    fn blob_identity_matches_git() {
        assert_eq!(
            blob_sha(b"hello\n"),
            "ce013625030ba8dba906f756967f9e9ca394464a"
        );
    }
}
#[cfg(test)]
mod publication_tests {
    use super::*;
    use std::{
        cell::{Cell, RefCell},
        io::Write,
    };
    fn publication(attempt: u64) -> Value {
        json!({"format":1,"kind":"full","origin":{"repository":"example/capntproto","run_id":1,"attempt":attempt,"commit":"b".repeat(40)},"source_id":"a".repeat(64),"status":"passed","note":"Synthetic test fixture, not a measurement.","tests":null,"charts":[]})
    }
    fn archive(name: &str, data: &Value) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        zip.start_file(name, zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(data.to_string().as_bytes()).unwrap();
        zip.finish().unwrap().into_inner()
    }
    struct Fake {
        initial: bool,
        ci: bool,
        attempt: Cell<usize>,
        calls: RefCell<Vec<(String, Value, String)>>,
        archive: Vec<u8>,
    }
    impl Api for Fake {
        fn download(&self, path: &str) -> Result<Vec<u8>> {
            if self.ci {
                let id: usize = path.split('/').nth(3).unwrap().parse()?;
                let mut data = publication(1);
                data["kind"] = json!(["cargo", "models", "fuzz", "benchmark"][id - 9]);
                return Ok(archive("publication.json", &data));
            }
            Ok(self.archive.clone())
        }
        fn content(&self, path: &str, _: &str) -> Result<Option<(Vec<u8>, String)>> {
            Ok(match path {
                "docs/reports/history.json" | "quality/reporting/history-seed.json" => {
                    let mut seed = render::empty_history();
                    seed["fuzz"] = json!([{"run_id":2,"attempt":1,"commit":"b".repeat(40),"date":"2026-09-28T10:00:00Z","url":"https://github.com/example/capntproto/actions/runs/2","conclusion":"failure","data":data::unavailable("fuzz","Synthetic fixture")}]);
                    Some((seed.to_string().into_bytes(), "e".repeat(40)))
                }
                "docs/reports.template.md" => Some((
                    b"# Synthetic template\n{{REPORTS}}".to_vec(),
                    "f".repeat(40),
                )),
                _ => None,
            })
        }
        fn request(&self, path: &str, body: Option<&Value>, method: &str) -> Result<Value> {
            let body = body.cloned().unwrap_or(Value::Null);
            self.calls
                .borrow_mut()
                .push((path.into(), body.clone(), method.into()));
            Ok(match path {
                "" => json!({"default_branch":"main"}),
                "/actions/runs/1" => {
                    json!({"id":1,"run_attempt":1,"path":if self.ci {".github/workflows/ci.yml"} else {".github/workflows/verification-coverage.yml"},"status":"completed","event":"push","head_branch":"main","head_repository":{"full_name":"example/capntproto"},"head_sha":"b".repeat(40),"created_at":"2026-09-28T10:00:00Z","conclusion":"success","html_url":"https://github.com/example/capntproto/actions/runs/1"})
                }
                "/git/ref/heads/main" => json!({"object":{"sha":"f".repeat(40)}}),
                "/git/ref/heads/reports" => {
                    if self.initial && self.attempt.get() == 0 {
                        return Err(HttpError(404).into());
                    }
                    json!({"object":{"sha":if self.attempt.get()==0{"c".repeat(40)}else{"d".repeat(40)}}})
                }
                "/actions/runs/1/artifacts?per_page=100" => {
                    if self.ci {
                        let artifacts: Vec<_> = ["cargo", "models", "fuzz", "benchmark"].into_iter().enumerate().map(|(i,lane)| json!({"id":i+9,"name":format!("readme-data-{lane}-1"),"expired":false,"size_in_bytes":1024})).collect();
                        json!({"artifacts":artifacts})
                    } else {
                        json!({"artifacts":[{"id":9,"name":"readme-data","expired":false,"size_in_bytes":self.archive.len()}]})
                    }
                }
                "/git/refs/heads/reports" | "/git/refs" => {
                    self.attempt.set(self.attempt.get() + 1);
                    if self.attempt.get() == 1 {
                        return Err(HttpError(409).into());
                    }
                    json!({})
                }
                "/git/blobs" | "/git/trees" | "/git/commits" => json!({"sha":"a".repeat(40)}),
                p if p.starts_with("/compare/") => {
                    json!({"merge_base_commit":{"sha":"b".repeat(40)}})
                }
                p if p.starts_with("/contents/docs/reports") => {
                    json!([{"path":"docs/reports/latency-p50.svg","sha":"e".repeat(40),"type":"file"},{"path":"docs/reports/keep.txt","sha":"f".repeat(40),"type":"file"}])
                }
                p if p.starts_with("/git/commits/") => json!({"tree":{"sha":"e".repeat(40)}}),
                _ => bail!("unexpected API request {path}"),
            })
        }
    }
    fn api(initial: bool, name: &str, attempt: u64) -> Fake {
        Fake {
            initial,
            ci: false,
            attempt: Cell::new(0),
            calls: Default::default(),
            archive: archive(name, &publication(attempt)),
        }
    }
    #[test]
    fn ci_publishes_all_four_lanes_in_one_atomic_update() {
        let mut api = api(true, "publication.json", 1);
        api.ci = true;
        execute(
            &api,
            &json!({"workflow_run":{"id":1}}),
            "example/capntproto",
        )
        .unwrap();
        let calls = api.calls.borrow();
        let histories: Vec<Value> = calls
            .iter()
            .filter(|(path, _, _)| path == "/git/blobs")
            .filter_map(|(_, body, _)| {
                serde_json::from_slice(&STANDARD.decode(body["content"].as_str().unwrap()).unwrap())
                    .ok()
            })
            .filter(|v: &Value| v.get("cargo").is_some() && v.get("benchmark").is_some())
            .collect();
        assert!(!histories.is_empty());
        for history in histories {
            for lane in ["cargo", "models", "fuzz"] {
                assert!(history[lane]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|r| r["run_id"] == 1 && r["data"]["kind"] == lane));
            }
            assert_eq!(history["benchmark"]["data"]["kind"], "benchmark");
        }
        drop(calls);
        let mut run = api.request("/actions/runs/1", None, "GET").unwrap();
        assert_eq!(
            artifact_data(&api, &run, "full").unwrap()["status"],
            "unavailable"
        );
        run["run_attempt"] = json!(2);
        assert_eq!(
            artifact_data(&api, &run, "cargo").unwrap()["status"],
            "unavailable"
        );
    }
    #[test]
    fn artifacts_are_bound_to_archive_path_and_attempt() {
        let mut run = json!({"id":1,"run_attempt":2,"head_sha":"b".repeat(40),"conclusion":"success","head_repository":{"full_name":"example/capntproto"}});
        assert!(artifact_data(&api(false, "../publication.json", 1), &run, "full").is_err());
        assert_eq!(
            artifact_data(&api(false, "publication.json", 1), &run, "full").unwrap()["status"],
            "unavailable"
        );
        assert_eq!(
            artifact_data(&api(false, "publication.json", 2), &run, "full").unwrap()["status"],
            "passed"
        );
        run["conclusion"] = json!("cancelled");
        assert_eq!(
            artifact_data(&api(false, "publication.json", 2), &run, "full").unwrap()["status"],
            "failed"
        );
    }
    #[test]
    fn atomic_publication_retries_and_touches_only_owned_report_paths() {
        for initial in [false, true] {
            let api = api(initial, "publication.json", 1);
            execute(
                &api,
                &json!({"workflow_run":{"id":1}}),
                "example/capntproto",
            )
            .unwrap();
            let calls = api.calls.borrow();
            assert!(!calls
                .iter()
                .any(|(path, _, method)| path == "/git/refs/heads/main" && method != "GET"));
            let trees: Vec<_> = calls
                .iter()
                .filter(|(path, _, _)| path == "/git/trees")
                .map(|(_, body, _)| body)
                .collect();
            assert_eq!(trees.len(), 2);
            let owned = render::owned();
            assert!(trees.iter().all(|tree| tree["tree"]
                .as_array()
                .unwrap()
                .iter()
                .all(|e| owned.contains(e["path"].as_str().unwrap()))));
            if initial {
                assert!(trees[0].get("base_tree").is_none());
                assert!(trees[0]["tree"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|e| !e["sha"].is_null()));
            } else {
                assert!(trees[0]["tree"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|e| e["path"] == "docs/reports/latency-p50.svg" && e["sha"].is_null()));
            }
            let commits: Vec<_> = calls
                .iter()
                .filter(|(path, _, _)| path == "/git/commits")
                .map(|(_, body, _)| body)
                .collect();
            assert_eq!(
                commits[0]["parents"],
                if initial {
                    json!([])
                } else {
                    json!(["c".repeat(40)])
                }
            );
            assert_eq!(commits[1]["parents"], json!(["d".repeat(40)]));
            for (path, body, _) in calls.iter() {
                if path == "/git/refs/heads/reports" {
                    assert_eq!(body["force"], false);
                }
                if path == "/git/refs" {
                    assert_eq!(body["ref"], "refs/heads/reports");
                }
            }
        }
    }
    #[test]
    fn insecure_redirects_are_rejected_without_following() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            let mut byte = [0];
            while !request.ends_with(b"\r\n\r\n") {
                stream.read_exact(&mut byte).unwrap();
                request.push(byte[0]);
            }
            stream.write_all(b"HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/foreign-storage\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
            String::from_utf8(request).unwrap()
        });
        let mut api = GitHub::new("example/capntproto", "fixture-token".into()).unwrap();
        api.base = format!("http://{address}");
        assert!(api
            .download("/artifact")
            .unwrap_err()
            .to_string()
            .contains("unsafe artifact redirect"));
        assert!(server
            .join()
            .unwrap()
            .to_lowercase()
            .contains("authorization: bearer fixture-token"));
    }
}
