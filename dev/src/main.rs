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
    /// Reject tracked build artifacts, merge conflicts and oversized Git blobs.
    CheckRepository,
    /// Run schema services and the distributed checkout application.
    Examples(dev::examples::Options),
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
    /// Prove shared RPC state transitions and the fixed arena, with mutation controls.
    Kani(dev::kani::Args),
    /// Verify source contracts and unbounded temporal proofs, with fault controls.
    #[command(subcommand)]
    Verus(dev::verus::Args),
    #[command(subcommand)]
    Wiki(dev::wiki::Args),
    #[command(subcommand)]
    Reports(dev::reports::Args),
    DigitaloceanBench(CleanupArgs),
    AflFuzz(dev::fuzz::Args),
    PackageFuzzReport,
    #[command(subcommand)]
    Models(dev::models::Args),
    #[command(subcommand)]
    Probe(dev::probes::Args),
}
#[derive(Parser)]
struct CleanupArgs {
    #[arg(value_enum)]
    action: CleanupAction,
    #[arg(long)]
    state: std::path::PathBuf,
}
#[derive(Clone, clap::ValueEnum)]
enum CleanupAction {
    Cleanup,
    Janitor,
}
fn main() -> Result<()> {
    let cli = Cli::parse();
    std::env::set_current_dir(dev::root())?;
    match cli.command {
        Command::CheckRepository => dev::checks::repository(&dev::root()),
        Command::Examples(options) => dev::examples::run(options),
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
        Command::Kani(args) => dev::kani::run(args),
        Command::Verus(args) => dev::verus::run(args),
        Command::Wiki(a) => dev::wiki::run(a),
        Command::Reports(a) => dev::reports::run(a),
        Command::DigitaloceanBench(a) => dev::cloud::run(dev::cloud::Args {
            action: match a.action {
                CleanupAction::Cleanup => dev::cloud::Action::Cleanup,
                CleanupAction::Janitor => dev::cloud::Action::Janitor,
            },
            state: a.state,
            trace_io: false,
            bundle: None,
            output: None,
        }),
        Command::AflFuzz(a) => dev::fuzz::run(a),
        Command::PackageFuzzReport => {
            println!("{}", dev::fuzz::package(&dev::root())?.display());
            Ok(())
        }
        Command::Models(a) => dev::models::run(a),
        Command::Probe(a) => dev::probes::run(a),
    }
}
