//! Exercise the infrastructure lifecycle with a fake API; never allocate a host.
use capntproto_test_support::verification as v;
#[test]
fn digitalocean_cleanup_and_cost_guards() {
    v::run(
        v::command("python3").arg("quality/tests/digitalocean_checks.py"),
        &v::root().join("target/verification/digitalocean.log"),
        0,
    )
    .unwrap();
}
