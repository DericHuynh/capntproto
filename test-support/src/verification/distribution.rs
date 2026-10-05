//! Deterministic source archives and isolated Cargo qualification.
use super::*;
use flate2::{write::GzEncoder, Compression};
use std::io::Write;

const DIRECTORIES: &[&str] = &[
    ".cargo",
    ".config",
    ".github",
    "quality",
    "dev",
    "crates",
    "src",
    "schemas",
    "tests",
    "test-support",
    "fuzz",
    "examples",
    "vendor",
    "scripts",
    "verification",
    "docs",
    "benchmarks",
    "research",
];
const FILES: &[&str] = &[
    ".gitignore",
    ".gitmodules",
    "Cargo.toml",
    "Cargo.lock",
    "build.rs",
    "rust-toolchain.toml",
    "README.md",
    "LICENSE",
    "THIRD_PARTY_NOTICES.md",
    "SECURITY.md",
    "CHANGELOG.md",
    "research/baseline/CapnpRpc.tla",
    "research/reports/README.md",
    "vendor/capnproto/CMakeLists.txt",
    "vendor/capnproto/LICENSE",
    "crates/capntproto-core/src/lib.rs",
    "crates/capntproto-core/LICENSE",
    "crates/capntproto-rpc/src/lib.rs",
    "crates/capntproto-rpc/LICENSE",
    "crates/capntproto-futures/src/lib.rs",
    "crates/capntproto-futures/LICENSE",
    "crates/capntproto-codegen/src/lib.rs",
    "crates/capntproto-codegen/LICENSE",
    "schemas/imports/capnp/schema.capnp",
    "schemas/imports/capnp/c++.capnp",
    "schemas/imports/capnp/stream.capnp",
];
const EXCLUDE: &[&str] = &[
    ".git",
    "target",
    "__pycache__",
    "node_modules",
    "states",
    ".DS_Store",
];
fn visit(base: &Path, dir: &Path, paths: &mut BTreeSet<PathBuf>, source_only: bool) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        if source_only && EXCLUDE.contains(&entry.file_name().to_string_lossy().as_ref()) {
            continue;
        }
        let path = entry.path();
        // Include the baseline model and frozen measurements explicitly below;
        // archived snapshots and local report logs are not source inputs.
        if source_only
            && (path == base.join("research/baseline") || path == base.join("research/reports"))
        {
            continue;
        }
        // First-party fuzz discoveries/builds are run artifacts. The Rust seed
        // matrix is bundled; inherited upstream corpus fixtures remain included.
        if source_only && (path == base.join("fuzz/artifacts") || path == base.join("fuzz/corpus"))
        {
            continue;
        }
        if entry.file_type()?.is_symlink() && !path.exists() {
            // Optional upstream build-system links can be dangling. They were
            // never source files and are not included in the archive.
            continue;
        }
        if !path.canonicalize()?.starts_with(base) {
            return Err(format!("source escapes root: {}", path.display()).into());
        }
        if entry.file_type()?.is_symlink() && path.is_dir() {
            // The tree already contains this directory at its real path.
            // Do not duplicate directory aliases or recurse through cycles.
            continue;
        }
        if entry.file_type()?.is_dir() {
            visit(base, &path, paths, source_only)?;
        } else if path.is_file() {
            paths.insert(path.strip_prefix(base)?.into());
        } else {
            return Err(format!("unsupported source entry: {}", path.display()).into());
        }
    }
    Ok(())
}
pub fn sources(base: &Path) -> Result<BTreeSet<PathBuf>> {
    let base = base.canonicalize()?;
    let mut paths: BTreeSet<_> = FILES.iter().map(PathBuf::from).collect();
    for name in DIRECTORIES {
        visit(&base, &base.join(name), &mut paths, true)?;
    }
    for entry in fs::read_dir(&base)? {
        let p = entry?.path();
        if p.extension().is_some_and(|e| e == "md") {
            paths.insert(p.strip_prefix(&base)?.into());
        }
    }
    for folder in [
        "research/reports/storage-benchmark",
        "research/reports/eae-integration",
        "research/reports/storage-next/2026-10-01",
        "research/reports/storage-resilience/2026-10-01",
        "research/reports/concurrency/2026-10-01",
        "research/reports/rpc-pipeline/2026-10-01",
    ] {
        let folder = base.join(folder);
        if folder.is_dir() {
            for entry in fs::read_dir(folder)? {
                let path = entry?.path();
                if path
                    .extension()
                    .is_some_and(|e| e == "json" || e == "jsonl")
                {
                    paths.insert(path.strip_prefix(&base)?.into());
                }
            }
        }
    }
    for p in &paths {
        if !base.join(p).canonicalize()?.starts_with(&base) || !base.join(p).is_file() {
            return Err(format!("invalid source {}", p.display()).into());
        }
    }
    Ok(paths)
}
pub fn source_hashes(base: &Path) -> Result<BTreeMap<PathBuf, String>> {
    sources(base)?
        .into_iter()
        .map(|p| Ok((p.clone(), sha256(fs::read(base.join(p))?))))
        .collect()
}

/// Runtime qualification covers executable inputs. The archive manifest still
/// covers every file, including prose and release-status documentation.
pub fn verification_hashes(base: &Path) -> Result<BTreeMap<PathBuf, String>> {
    Ok(source_hashes(base)?
        .into_iter()
        .filter(|(path, _)| {
            !path.starts_with("docs/reports")
                && !path.extension().is_some_and(|e| e == "md" || e == "rst")
        })
        .collect())
}
#[derive(Serialize, Deserialize)]
pub struct Manifest {
    pub version: u32,
    pub package: String,
    pub package_version: String,
    pub qualified_runtime: bool,
    pub modes: BTreeMap<PathBuf, u32>,
    pub sources: BTreeMap<PathBuf, String>,
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Bundle {
    pub sha256: String,
    pub files: usize,
    pub bytes: u64,
    pub qualified_runtime: bool,
}
pub fn build(base: &Path, destination: &Path, qualified: bool) -> Result<Bundle> {
    let paths = sources(base)?;
    let hashes = source_hashes(base)?;
    if qualified {
        let evidence: serde_json::Value = serde_json::from_slice(&fs::read(
            base.join("target/release-qualification/cargo-qualification.json"),
        )?)?;
        let checked: BTreeMap<PathBuf, String> =
            serde_json::from_value(evidence["sources"].clone())?;
        if verification_hashes(base)? != checked {
            return Err("source qualification is stale; run cargo nextest run --test release isolated_release_qualification -- --ignored --exact".into());
        }
    }
    let mut manifest = Manifest {
        version: 1,
        package: "capntproto".into(),
        package_version: env!("CARGO_PKG_VERSION").into(),
        qualified_runtime: qualified,
        modes: BTreeMap::new(),
        sources: hashes,
    };
    let mut contents = BTreeMap::new();
    for path in paths {
        let mode = {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if fs::metadata(base.join(&path))?.permissions().mode() & 0o111 != 0 {
                    0o755
                } else {
                    0o644
                }
            }
            #[cfg(not(unix))]
            {
                0o644
            }
        };
        manifest.modes.insert(path.clone(), mode);
        contents.insert(path.clone(), fs::read(base.join(path))?);
    }
    if qualified {
        for name in [
            "cargo-qualification.json",
            "cargo-tests.log",
            "cargo-doctests.log",
            "cargo-clippy.log",
        ] {
            let name = PathBuf::from("target/release-qualification").join(name);
            let bytes = fs::read(base.join(&name))?;
            manifest.modes.insert(name.clone(), 0o644);
            manifest.sources.insert(name.clone(), sha256(&bytes));
            contents.insert(name, bytes);
        }
    }
    let mut json = serde_json::to_vec_pretty(&manifest)?;
    json.push(b'\n');
    contents.insert(PathBuf::from("SOURCE_MANIFEST.json"), json);
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    let gzip = GzEncoder::new(File::create(destination)?, Compression::best());
    let mut archive = tar::Builder::new(gzip);
    for (name, bytes) in &contents {
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        header.set_mode(*manifest.modes.get(name).unwrap_or(&0o644));
        header.set_mtime(0);
        header.set_uid(0);
        header.set_gid(0);
        header.set_cksum();
        archive.append_data(
            &mut header,
            PathBuf::from(format!("capntproto-{}-source", manifest.package_version)).join(name),
            bytes.as_slice(),
        )?;
    }
    archive.into_inner()?.finish()?.flush()?;
    let result = Bundle {
        sha256: sha256(fs::read(destination)?),
        files: contents.len(),
        bytes: fs::metadata(destination)?.len(),
        qualified_runtime: qualified,
    };
    fs::write(
        destination.with_extension("gz.json"),
        serde_json::to_vec_pretty(&result)?,
    )?;
    Ok(result)
}
pub fn unpack(archive: &Path, destination: &Path) -> Result<PathBuf> {
    fs::create_dir_all(destination)?;
    let mut archive = tar::Archive::new(GzDecoder::new(File::open(archive)?));
    for entry in archive.entries()? {
        let mut entry = entry?;
        if !entry.header().entry_type().is_file() {
            return Err("source archive contains non-file entry".into());
        }
        if !entry.unpack_in(destination)? {
            return Err("archive path escapes extraction root".into());
        }
    }
    let roots: Vec<_> = fs::read_dir(destination)?.collect::<std::io::Result<_>>()?;
    if roots.len() != 1 {
        return Err("expected one archive root".into());
    }
    let base = roots[0].path();
    let manifest: Manifest = serde_json::from_slice(&fs::read(base.join("SOURCE_MANIFEST.json"))?)?;
    let mut actual = BTreeSet::new();
    // Check every extracted file, including explicitly bundled baseline models
    // and qualification evidence under target/. Source filters do not apply.
    visit(&base, &base, &mut actual, false)?;
    let mut expected: BTreeSet<_> = manifest.sources.keys().cloned().collect();
    expected.insert("SOURCE_MANIFEST.json".into());
    if actual != expected {
        return Err("archive file inventory mismatch".into());
    }
    for (name, hash) in manifest.sources {
        if sha256(fs::read(base.join(&name))?) != hash {
            return Err(format!("archive content mismatch {}", name.display()).into());
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if fs::metadata(base.join(&name))?.permissions().mode() & 0o777 != manifest.modes[&name]
            {
                return Err(format!("archive mode mismatch {}", name.display()).into());
            }
        }
    }
    Ok(base)
}
