//! Repository development tools. No production crate depends on this crate.
#![forbid(unsafe_code)]
pub mod checks;
pub mod cloud;
pub mod fuzz;
#[cfg(feature = "quic-interop")]
pub mod interop;
pub mod models;
pub mod probes;
pub mod process;
pub mod reports;
pub mod wiki;
use anyhow::{Context, Result};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs,
    path::{Path, PathBuf},
};
pub fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_owned()
}
pub fn hash(data: impl AsRef<[u8]>) -> String {
    hex::encode(Sha256::digest(data.as_ref()))
}
pub fn file_hash(path: impl AsRef<Path>) -> Result<String> {
    Ok(hash(fs::read(path.as_ref()).with_context(|| {
        format!("read {}", path.as_ref().display())
    })?))
}
pub fn read_json(path: impl AsRef<Path>) -> Result<Value> {
    Ok(serde_json::from_slice(&fs::read(path.as_ref())?)?)
}
pub fn write(path: impl AsRef<Path>, data: impl AsRef<[u8]>) -> Result<()> {
    let path = path.as_ref();
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, data)?;
    Ok(())
}
pub fn write_json(path: impl AsRef<Path>, value: &impl serde::Serialize) -> Result<()> {
    write(path, format!("{}\n", serde_json::to_string_pretty(value)?))
}
pub fn remove(path: impl AsRef<Path>) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}
pub fn string<'a>(v: &'a Value, name: &str) -> Result<&'a str> {
    v[name]
        .as_str()
        .with_context(|| format!("missing string {name}"))
}
pub fn number(v: &Value, name: &str) -> Result<u64> {
    v[name]
        .as_u64()
        .with_context(|| format!("missing nonnegative integer {name}"))
}
