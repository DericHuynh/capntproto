//! Full native reference suites remain correctness checks, outside LLVM coverage.
use capntproto_quality::{coverage, Runner};
use capntproto_test_support::verification as v;
#[test]
fn complete_cpp_reference_suites() {
    // A second workspace/coverage run must not replace evidence or enter the
    // shared reference build while the first invocation is still using it.
    let _run = v::lock(&v::root().join("target/verification/cpp-complete.lock")).unwrap();
    let output = v::root().join("target/verification/cpp-complete");
    let mut runner = Runner::new("cpp-reference", &output).unwrap();
    let result = coverage::cpp(&mut runner);
    runner.finish(result).unwrap();
}
