//! Run the supported Tokio/rustls applications locally or in CI.
use anyhow::Result;
use clap::Args;
use std::process::Command;
#[derive(Args)]
pub struct Options {}
pub fn run(_: Options) -> Result<()> {
    let cancel = crate::process::Cancellation::install()?;
    for example in ["service", "configurations", "kv", "checkout"] {
        crate::process::live(
            Command::new("cargo").args([
                "test",
                "--locked",
                "--manifest-path",
                &format!("examples/{example}/Cargo.toml"),
            ]),
            1800,
            &cancel,
        )?;
    }
    Ok(())
}
