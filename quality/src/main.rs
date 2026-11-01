use capntproto_quality::{coverage, qualification, report, Result, Runner};
use std::path::Path;
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        return Err("usage: capntproto-quality LANE OUTPUT [REQUIRED_LANES | BUNDLE_PATH]".into());
    }
    if args[0] == "report" {
        return report::generate(
            Path::new(&args[1]),
            args.get(2)
                .map(String::as_str)
                .unwrap_or("coverage,security"),
        );
    }
    let mut r = Runner::new(&args[0], Path::new(&args[1]))?;
    let result = match args[0].as_str() {
        "coverage" => coverage::collect(&mut r),
        "cargo" => capntproto_quality::lanes::cargo(&mut r),
        "qualification" => qualification(&mut r),
        "models" => capntproto_quality::lanes::models(&mut r),
        "memory" => r
            .cargo(
                "miri-and-mutations",
                &[
                    "nextest",
                    "run",
                    "--locked",
                    "--test",
                    "memory_safety",
                    "--test",
                    "guard_quality",
                ],
            )
            .map(|_| ()),
        "security" => {
            let mut failures = Vec::new();
            for (name, manifest) in [
                ("root", "Cargo.lock"),
                ("fuzz", "fuzz/Cargo.lock"),
                ("downstream-example", "examples/downstream/Cargo.lock"),
                ("kv-service-example", "examples/kv/Cargo.lock"),
                ("tls-example", "examples/configurations/Cargo.lock"),
                ("checkout-example", "examples/checkout/Cargo.lock"),
                ("runtime-benchmark", "benchmarks/runtimes/Cargo.lock"),
                ("storage-benchmark", "benchmarks/storage/Cargo.lock"),
                ("concurrency-benchmark", "benchmarks/concurrency/Cargo.lock"),
                ("miri", "verification/miri/Cargo.lock"),
                (
                    "upstream-rpc-interop",
                    "verification/upstream-rpc/Cargo.lock",
                ),
                (
                    "instructions-benchmark",
                    "benchmarks/instructions/Cargo.lock",
                ),
            ] {
                if let Err(error) =
                    r.cargo(name, &["audit", "--file", manifest, "--deny", "warnings"])
                {
                    failures.push(format!("{name}: {error}"));
                }
            }
            if failures.is_empty() {
                Ok(())
            } else {
                Err(failures.join("\n").into())
            }
        }
        _ => Err("unknown quality lane".into()),
    };
    r.finish(result)
}
