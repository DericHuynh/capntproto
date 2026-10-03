//! Public report contracts join the ordinary workspace test command.
#[test]
fn readme_collection_history_and_publication_contracts() {
    use capntproto_test_support::verification as v;
    let python = if cfg!(windows) { "python" } else { "python3" };
    v::run(
        v::command(python)
            .env("PYTHONPATH", v::root().join("quality"))
            .args(["-m", "unittest", "reporting.test_reporting", "-v"]),
        &v::root().join("target/verification/readme-reporting.log"),
        0,
    )
    .unwrap();
}
