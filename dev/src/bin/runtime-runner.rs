use capntproto_dev::runtime_bench::{run, Args};
use clap::Parser;
fn main() -> anyhow::Result<()> {
    run(Args::parse())
}
