//! Project documentation validation and owned, reproducible wiki exports.
use anyhow::{ensure, Result};
use clap::Subcommand;
use regex::Regex;
use serde_json::json;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Component, Path, PathBuf},
};
const FORMAT: &str = "capntproto-wiki-export-v1";
const MANIFEST: &str = "wiki-export.json";
#[derive(Subcommand)]
pub enum Args {
    Check,
    Build {
        #[arg(long, default_value = "target/wiki")]
        output: PathBuf,
        #[arg(long, default_value = "DericHuynh/capntproto")]
        repository: String,
        #[arg(long, default_value = "main")]
        source_ref: String,
    },
}
pub fn run(args: Args) -> Result<()> {
    let root = crate::root();
    match args {
        Args::Check => {
            check(&root)?;
            println!("Validated {} project documents.", documents(&root)?.len());
        }
        Args::Build {
            output,
            repository,
            source_ref,
        } => println!(
            "Exported {} wiki files. No publication performed.",
            build(&root, &output, &repository, &source_ref)?
        ),
    }
    Ok(())
}
fn re(pattern: &str) -> Regex {
    static PATTERNS: std::sync::OnceLock<std::sync::Mutex<BTreeMap<String, Regex>>> =
        std::sync::OnceLock::new();
    PATTERNS
        .get_or_init(Default::default)
        .lock()
        .expect("pattern cache")
        .entry(pattern.to_owned())
        .or_insert_with(|| Regex::new(pattern).expect("constant expression"))
        .clone()
}
fn mask(text: &str, inline: bool) -> String {
    let mut bytes = text.as_bytes().to_vec();
    let mut offset = 0;
    let mut fence: Option<(u8, usize)> = None;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start_matches(' ');
        let indent = line.len() - trimmed.len();
        let marker = trimmed
            .as_bytes()
            .first()
            .copied()
            .filter(|c| *c == b'`' || *c == b'~');
        let n = marker.map_or(0, |c| trimmed.bytes().take_while(|b| *b == c).count());
        let is_marker = indent <= 3 && n >= 3;
        let hidden = fence.is_some() || is_marker || indent >= 4 || line.starts_with('\t');
        if let Some((ch, len)) = fence {
            if is_marker && marker == Some(ch) && n >= len && trimmed[n..].trim().is_empty() {
                fence = None;
            }
        } else if is_marker {
            fence = Some((marker.unwrap(), n));
        }
        if hidden {
            for b in &mut bytes[offset..offset + line.len()] {
                if *b != b'\n' {
                    *b = b' ';
                }
            }
        }
        offset += line.len();
    }
    let visible = String::from_utf8(bytes.clone()).expect("masked UTF-8");
    for m in re(r"(?s)<!--.*?-->").find_iter(&visible) {
        for b in &mut bytes[m.range()] {
            if *b != b'\n' {
                *b = b' ';
            }
        }
    }
    if inline {
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] != b'`' {
                i += 1;
                continue;
            }
            let n = bytes[i..].iter().take_while(|b| **b == b'`').count();
            let mut j = i + n;
            let mut end = None;
            while j < bytes.len() {
                if bytes[j] == b'`' {
                    let len = bytes[j..].iter().take_while(|b| **b == b'`').count();
                    if len == n {
                        end = Some(j + n);
                        break;
                    }
                    j += len;
                } else {
                    j += 1;
                }
            }
            if let Some(end) = end {
                bytes[i..end].fill(b' ');
                i = end;
            } else {
                i += n;
            }
        }
    }
    String::from_utf8(bytes).expect("masked UTF-8")
}
pub fn links(text: &str) -> Vec<(std::ops::Range<usize>, String)> {
    let expression = re(
        r#"!?\[[^\]\n]*\]\(\s*(?P<inline><[^>\n]+>|(?:[^\s()\\]|\\.|\([^()\n]*\))+)(?:\s+["'][^\n]*?["'])?\s*\)|(?m:^ {0,3}\[[^\]\n]+\]:\s*(?P<reference><[^>\n]+>|\S+))"#,
    );
    expression
        .captures_iter(&mask(text, true))
        .map(|c| {
            let m = c.name("inline").or_else(|| c.name("reference")).unwrap();
            let range = if text.as_bytes()[m.start()] == b'<' {
                m.start() + 1..m.end() - 1
            } else {
                m.range()
            };
            let url = text[range.clone()].to_owned();
            (range, url)
        })
        .collect()
}
fn decode(s: &str) -> String {
    html_escape::decode_html_entities(s).into_owned()
}
pub fn anchors(text: &str) -> BTreeSet<String> {
    let text = mask(text, false);
    let mut result = BTreeSet::new();
    for cap in re(r#"<a\s+[^>]*?(?:id|name)=["']([^"']+)"#).captures_iter(&text) {
        result.insert(cap[1].to_owned());
    }
    let mut used = BTreeSet::new();
    let mut previous = "";
    for line in text.lines() {
        let heading = re(r"^ {0,3}#{1,6}\s+(.+?)\s*#*\s*$")
            .captures(line)
            .map(|c| c[1].to_owned())
            .or_else(|| {
                (re(r"^ {0,3}(?:=+|-+)\s*$").is_match(line) && !previous.trim().is_empty())
                    .then(|| previous.trim().to_owned())
            });
        previous = line;
        if let Some(heading) = heading {
            let heading = re(r"!?\[([^]]*)\]\([^)]*\)").replace_all(&heading, "$1");
            let heading = decode(&re(r"<[^>]*>").replace_all(&heading, "")).to_lowercase();
            let slug = re(r"[^\w\-\s]")
                .replace_all(&heading, "")
                .replace([' ', '\t'], "-");
            let mut name = slug.clone();
            let mut suffix = 0;
            while used.contains(&name) {
                suffix += 1;
                name = format!("{slug}-{suffix}");
            }
            used.insert(name.clone());
            result.insert(name);
        }
    }
    result
}
fn normalize(path: &Path) -> Result<PathBuf> {
    let mut result = PathBuf::new();
    for part in path.components() {
        match part {
            Component::ParentDir => {
                ensure!(result.pop(), "path escapes filesystem root");
            }
            Component::CurDir => {}
            other => result.push(other.as_os_str()),
        }
    }
    Ok(result)
}
fn resolve_existing(path: &Path) -> Result<PathBuf> {
    if path.exists() {
        return Ok(path.canonicalize()?);
    }
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("invalid path"))?;
    Ok(resolve_existing(parent)?.join(
        path.file_name()
            .ok_or_else(|| anyhow::anyhow!("invalid path"))?,
    ))
}
fn target(
    root: &Path,
    source: &Path,
    destination: &str,
) -> Result<Option<(PathBuf, String, String)>> {
    let url = decode(destination);
    if url.starts_with("//") || re(r"^[a-zA-Z][a-zA-Z0-9+.-]*:").is_match(&url) {
        return Ok(None);
    }
    let (path, fragment) = url.split_once('#').unwrap_or((&url, ""));
    let (path, query) = path.split_once('?').unwrap_or((path, ""));
    let path = percent_encoding::percent_decode_str(path).decode_utf8()?;
    let resolved = resolve_existing(&normalize(&if path.is_empty() {
        source.to_owned()
    } else {
        source.parent().unwrap().join(path.as_ref())
    })?)?;
    ensure!(
        resolved.starts_with(root.canonicalize()?),
        "link escapes repository: {destination}"
    );
    Ok(Some((
        resolved,
        percent_encoding::percent_decode_str(fragment)
            .decode_utf8()?
            .into_owned(),
        query.to_owned(),
    )))
}
pub fn documents(root: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = BTreeSet::new();
    for entry in std::fs::read_dir(root)? {
        let path = entry?.path();
        if path.extension().is_some_and(|s| s == "md") {
            paths.insert(path);
        }
    }
    for dir in ["docs", ".github", "crates", "benchmarks", "dev"] {
        let dir = root.join(dir);
        if !dir.exists() {
            continue;
        }
        for e in walkdir::WalkDir::new(dir)
            .into_iter()
            .filter_entry(|e| !matches!(e.file_name().to_str(), Some("target" | "node_modules")))
        {
            let e = e?;
            if e.file_type().is_file() && e.path().extension().is_some_and(|s| s == "md") {
                paths.insert(e.into_path());
            }
        }
    }
    for dir in ["research/reports", "vendor"] {
        let dir = root.join(dir);
        if !dir.exists() {
            continue;
        }
        for entry in std::fs::read_dir(&dir)? {
            let path = entry?.path();
            let path = if dir.ends_with("vendor") {
                path.join("REPROTO.md")
            } else {
                path
            };
            if path.is_file() && path.extension().is_some_and(|s| s == "md") {
                paths.insert(path);
            }
        }
    }
    Ok(paths.into_iter().collect())
}
pub fn check(root: &Path) -> Result<()> {
    let mut errors = Vec::new();
    let mut anchor_cache = BTreeMap::new();
    for source in documents(root)? {
        if source.ends_with("docs/reports.template.md") {
            continue;
        }
        let text = std::fs::read_to_string(&source)?;
        for (range, url) in links(&text) {
            let result = (|| -> Result<()> {
                if let Some((path, fragment, _)) = target(root, &source, &url)? {
                    if !path.exists() {
                        let relative = path.strip_prefix(root.canonicalize()?)?.to_string_lossy();
                        ensure!(
                            ["target/", "dist/", "vendor/capnproto/"]
                                .iter()
                                .any(|p| relative.starts_with(p)),
                            "missing target {url}"
                        );
                    } else if !fragment.is_empty()
                        && path
                            .extension()
                            .is_some_and(|e| e.eq_ignore_ascii_case("md"))
                    {
                        if !anchor_cache.contains_key(&path) {
                            anchor_cache
                                .insert(path.clone(), anchors(&std::fs::read_to_string(&path)?));
                        }
                        ensure!(
                            anchor_cache[&path].contains(&fragment),
                            "missing anchor {url}"
                        );
                    }
                }
                Ok(())
            })();
            if let Err(e) = result {
                errors.push(format!(
                    "{}:{}: {e}",
                    source.display(),
                    text[..range.start].bytes().filter(|c| *c == b'\n').count() + 1
                ));
            }
        }
    }
    let wiki = root.join("docs/wiki");
    for name in ["Home.md", "_Sidebar.md", "_Footer.md"] {
        if !wiki.join(name).is_file() {
            errors.push(format!("missing wiki navigation file: {name}"));
        }
    }
    let sidebar = wiki.join("_Sidebar.md");
    let mut linked = BTreeSet::new();
    if sidebar.exists() {
        for (_, url) in links(&std::fs::read_to_string(&sidebar)?) {
            if let Ok(Some((path, _, _))) = target(root, &sidebar, &url) {
                linked.insert(path);
            }
        }
    }
    let mut seen = BTreeSet::new();
    if wiki.exists() {
        for entry in std::fs::read_dir(&wiki)? {
            let path = entry?.path();
            if path.extension().is_none_or(|s| s != "md") {
                continue;
            }
            let name = path.file_name().unwrap().to_string_lossy();
            if !seen.insert(name.to_lowercase()) {
                errors.push(format!("duplicate wiki page name: {name}"));
            }
            if !name.starts_with('_') && !linked.contains(&path.canonicalize()?) {
                errors.push(format!("wiki page absent from sidebar: {name}"));
            }
        }
    }
    ensure!(errors.is_empty(), "{}", errors.join("\n"));
    Ok(())
}
fn encode(s: &str, slash: bool) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) || (slash && b == b'/') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}
fn export_url(
    root: &Path,
    source: &Path,
    url: &str,
    repository: &str,
    reference: &str,
) -> Result<String> {
    if url.starts_with('#') {
        return Ok(url.into());
    }
    let Some((path, _, query)) = target(root, source, url)? else {
        return Ok(url.into());
    };
    let relative = path.strip_prefix(root.canonicalize()?)?;
    let base = format!("https://github.com/{repository}");
    let mut result = if relative.parent() == Some(Path::new("docs/wiki"))
        && relative.extension().is_some_and(|s| s == "md")
    {
        format!(
            "{base}/wiki/{}",
            encode(&relative.file_stem().unwrap().to_string_lossy(), false)
        )
    } else {
        format!(
            "{base}/{}/{}/{}",
            if path.is_dir() { "tree" } else { "blob" },
            encode(reference, false),
            encode(&relative.to_string_lossy(), true)
        )
    };
    if !query.is_empty() {
        result.push('?');
        result.push_str(&query);
    }
    if let Some((_, fragment)) = url.split_once('#') {
        result.push('#');
        result.push_str(fragment);
    }
    Ok(result)
}
pub fn build(root: &Path, output: &Path, repository: &str, reference: &str) -> Result<usize> {
    ensure!(
        re(r"^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$").is_match(repository),
        "repository must be owner/name"
    );
    ensure!(
        !reference.is_empty()
            && !reference
                .chars()
                .any(|c| c.is_whitespace() || c.is_control()),
        "invalid source ref"
    );
    check(root)?;
    let output = std::path::absolute(output)?;
    for ancestor in output.ancestors() {
        ensure!(
            !ancestor.is_symlink(),
            "output path must not contain symlinks"
        );
    }
    let output = normalize(&output)?;
    for path in documents(root)? {
        ensure!(
            !path.canonicalize()?.starts_with(&output),
            "output must not contain source documents"
        );
    }
    let mut previous = BTreeMap::<String, String>::new();
    if output.exists() && std::fs::read_dir(&output)?.next().is_some() {
        let manifest = output.join(MANIFEST);
        ensure!(
            !manifest.is_symlink() && manifest.is_file(),
            "nonempty output is not an owned wiki export"
        );
        let data = crate::read_json(&manifest)?;
        ensure!(
            data["format"] == FORMAT,
            "unrecognized wiki export manifest"
        );
        previous = serde_json::from_value(data["pages"].clone())?;
        for (name, digest) in &previous {
            ensure!(
                Path::new(name)
                    .file_name()
                    .is_some_and(|n| n == name.as_str())
                    && name.ends_with(".md"),
                "invalid page name in manifest"
            );
            let page = output.join(name);
            ensure!(
                !page.is_symlink() && page.is_file() && crate::file_hash(&page)? == *digest,
                "previous export changed: {name}; use a fresh output directory"
            );
        }
        for entry in std::fs::read_dir(&output)? {
            let name = entry?.file_name().to_string_lossy().into_owned();
            ensure!(
                name == MANIFEST || previous.contains_key(&name),
                "output contains unrelated files"
            );
        }
    }
    let mut rendered = BTreeMap::new();
    for entry in std::fs::read_dir(root.join("docs/wiki"))? {
        let path = entry?.path();
        if path.extension().is_none_or(|s| s != "md") {
            continue;
        }
        let mut body = std::fs::read_to_string(&path)?;
        for (range, url) in links(&body).into_iter().rev() {
            body.replace_range(
                range,
                &export_url(root, &path, &url, repository, reference)?,
            );
        }
        rendered.insert(
            path.file_name().unwrap().to_string_lossy().into_owned(),
            body,
        );
    }
    let mut pages = BTreeMap::new();
    for (name, body) in &rendered {
        crate::write(output.join(name), body)?;
        pages.insert(name, crate::hash(body));
    }
    for name in previous.keys() {
        if !rendered.contains_key(name) {
            std::fs::remove_file(output.join(name))?;
        }
    }
    crate::write_json(
        output.join(MANIFEST),
        &json!({"format":FORMAT,"repository":repository,"source_ref":reference,"pages":pages}),
    )?;
    Ok(rendered.len())
}
