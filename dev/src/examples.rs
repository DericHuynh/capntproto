//! Distributed checkout, browser, embedded and runtime integration tests.
use anyhow::Result;
use clap::Args;
use std::process::Command;
#[derive(Args)]
pub struct Options {
    #[arg(long, conflicts_with = "browser_only")]
    pub native_only: bool,
    #[arg(long, conflicts_with = "native_only")]
    pub browser_only: bool,
}
pub fn run(options: Options) -> Result<()> {
    let mut command = Command::new("node");
    command.arg(crate::root().join("examples/integration/run.mjs"));
    if options.native_only {
        command.arg("--native-only");
    }
    if options.browser_only {
        command.arg("--browser-only");
    }
    crate::process::live(
        &mut command,
        1800,
        &crate::process::Cancellation::install()?,
    )
}
