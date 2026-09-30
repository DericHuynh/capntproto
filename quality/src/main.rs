use reproto_quality::{benchmark, coverage, qualification, report, Result, Runner};
use std::path::Path;
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    if args.len() < 2 {
        return Err("usage: reproto-quality LANE OUTPUT [REQUIRED_LANES | BUNDLE_PATH]".into());
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
        "qualification" => qualification(&mut r),
        "benchmark-build" => benchmark::bundle(&mut r),
        "benchmark" => benchmark::import(
            &mut r,
            Path::new(args.get(2).ok_or("missing bundle directory")?),
        ),
        "memory" => r
            .cargo(
                "miri-and-mutations",
                &[
                    "test",
                    "--locked",
                    "--test",
                    "memory_safety",
                    "--test",
                    "guard_quality",
                ],
            )
            .map(|_| ()),
        "fuzz" => r
            .cargo("asan", &["test", "--locked", "--test", "noise_fuzz"])
            .map(|_| ()),
        "security" => (|| {
            for (name, manifest) in [
                ("root", "Cargo.lock"),
                ("fuzz", "fuzz/Cargo.lock"),
                ("rpc-benchmark", "benchmarks/rpc/Cargo.lock"),
                (
                    "instructions-benchmark",
                    "benchmarks/instructions/Cargo.lock",
                ),
            ] {
                r.cargo(name, &["audit", "--file", manifest, "--deny", "warnings"])?;
            }
            Ok(())
        })(),
        _ => Err("unknown quality lane".into()),
    };
    r.finish(result)
}
