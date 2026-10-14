use anyhow::Result;
use capntproto_dev as dev;
use clap::{Parser, Subcommand};
#[derive(Parser)]
#[command(about = "Capntproto development and CI tools")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    /// Test the production quiche transport against independent s2n-quic peers.
    #[cfg(feature = "quic-interop")]
    CheckQuicInterop {
        #[arg(long, default_value = "target/debug/examples/quic-interop")]
        binary: std::path::PathBuf,
    },
    /// Reject tracked build artifacts, merge conflicts and oversized Git blobs.
    CheckRepository,
    /// Verify dependency inventories embedded by cargo-auditable.
    CheckAuditable(dev::checks::AuditArgs),
    /// Run nextest with a separate JUnit report. Prefix and selection are separated by --.
    Nextest {
        #[arg(trailing_var_arg = true, allow_hyphen_values = true, required = true)]
        args: Vec<String>,
    },
    /// Enforce source-bound unsafe documentation debt.
    CheckUnsafe,
    /// Verify workflow trigger and trust boundaries.
    CheckWorkflows,
    /// Build and profile the complete portable TCP client; optionally train LLVM PGO.
    RpcInstructions(dev::rpc_instructions::Args),
    /// Prove the portable engine and fixed arena, with production mutation controls.
    Kani(dev::kani::Args),
    /// Verify source contracts and unbounded temporal proofs, with fault controls.
    #[command(subcommand)]
    Verus(dev::verus::Args),
    #[command(subcommand)]
    Wiki(dev::wiki::Args),
    #[command(subcommand)]
    Reports(dev::reports::Args),
    DigitaloceanBench(dev::cloud::Args),
    AflFuzz(dev::fuzz::Args),
    PackageFuzzReport,
    #[command(subcommand)]
    Models(dev::models::Args),
    #[command(subcommand)]
    Probe(dev::probes::Args),
}
fn main() -> Result<()> {
    let cli = Cli::parse();
    std::env::set_current_dir(dev::root())?;
    match cli.command {
        #[cfg(feature = "quic-interop")]
        Command::CheckQuicInterop { binary } => dev::interop::run(&binary),
        Command::CheckRepository => dev::checks::repository(&dev::root()),
        Command::CheckAuditable(a) => dev::checks::audit(&a),
        Command::Nextest { args } => {
            let code = dev::checks::nextest(&args)?;
            if code != 0 {
                std::process::exit(code);
            }
            Ok(())
        }
        Command::CheckUnsafe => dev::checks::unsafe_code(),
        Command::CheckWorkflows => dev::checks::workflows(),
        Command::RpcInstructions(args) => dev::rpc_instructions::run(args),
        Command::Kani(args) => dev::kani::run(args),
        Command::Verus(args) => dev::verus::run(args),
        Command::Wiki(a) => dev::wiki::run(a),
        Command::Reports(a) => dev::reports::run(a),
        Command::DigitaloceanBench(a) => dev::cloud::run(a),
        Command::AflFuzz(a) => dev::fuzz::run(a),
        Command::PackageFuzzReport => {
            println!("{}", dev::fuzz::package(&dev::root())?.display());
            Ok(())
        }
        Command::Models(a) => dev::models::run(a),
        Command::Probe(a) => dev::probes::run(a),
    }
}
