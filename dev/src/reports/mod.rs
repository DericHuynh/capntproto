//! Source-bound CI evidence, standalone SVG graphs, and isolated report publication.
mod benchmarks;
pub mod data;
mod publish;
pub mod render;
mod verification;
use anyhow::{ensure, Result};
use clap::{Subcommand, ValueEnum};
use std::path::PathBuf;
#[derive(Clone, Copy, ValueEnum)]
pub enum Kind {
    Full,
    Cargo,
    Coverage,
    Models,
    Fuzz,
    Benchmark,
}
impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Cargo => "cargo",
            Self::Coverage => "coverage",
            Self::Models => "models",
            Self::Fuzz => "fuzz",
            Self::Benchmark => "benchmark",
        }
    }
}
#[derive(Subcommand)]
pub enum Args {
    Collect {
        #[arg(long)]
        input: PathBuf,
        #[arg(long, value_enum)]
        kind: Kind,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        failures_output: Option<PathBuf>,
        #[arg(long)]
        report_output: Option<PathBuf>,
        #[arg(long)]
        benchmark_manifest: Option<PathBuf>,
    },
    Render {
        #[arg(long)]
        root: Option<PathBuf>,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        history: Option<PathBuf>,
    },
    Publish {
        #[arg(long)]
        event: PathBuf,
    },
}
pub fn run(args: Args) -> Result<()> {
    match args {
        Args::Collect {
            input,
            kind,
            output,
            failures_output,
            report_output,
            benchmark_manifest,
        } => {
            crate::remove(&output)?;
            if let Some(path) = &failures_output {
                crate::remove(path)?;
            }
            let mut value = data::collect(&input, kind.name())?;
            if matches!(kind, Kind::Benchmark) {
                value = benchmarks::collect(&input, benchmark_manifest.as_deref(), value)?;
                data::validate_publication(&value)?;
            } else {
                ensure!(
                    benchmark_manifest.is_none(),
                    "build manifest is only valid for benchmark collection"
                );
            }
            crate::write_json(&output, &value)?;
            if let Some(path) = failures_output {
                crate::write(
                    &path,
                    format!(
                        "# Failed workspace tests\n\n{}",
                        render::failure_details(&value)?
                    ),
                )?;
            }
            if let Some(path) = report_output {
                render::artifact(&value, &path)?;
            }
            Ok(())
        }
        Args::Render {
            root,
            output,
            history,
        } => {
            let root = root.unwrap_or_else(crate::root);
            std::fs::create_dir_all(&output)?;
            ensure!(
                output.canonicalize()? != root.canonicalize()?,
                "render into a separate output directory, not the source checkout"
            );
            let data = crate::read_json(
                history.unwrap_or_else(|| root.join("quality/reporting/history-seed.json")),
            )?;
            render::render(
                &std::fs::read_to_string(root.join("docs/reports.template.md"))?,
                &data,
                &output,
            )?;
            Ok(())
        }
        Args::Publish { event } => publish::publish(&event),
    }
}
