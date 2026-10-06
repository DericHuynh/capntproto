use capntproto_quality::{Evidence, Runner};
use capntproto_test_support::verification as v;
use serde_json::Value;
use std::{fs, path::Path, process::Command};

fn saved(directory: &Path) -> Evidence {
    serde_json::from_slice(&fs::read(directory.join("evidence.json")).unwrap()).unwrap()
}

#[test]
fn nextest_evidence_belongs_to_each_invocation_including_failures() {
    let directory = tempfile::tempdir().unwrap();
    let fixture = directory.path().join("fixture");
    fs::create_dir_all(fixture.join("src")).unwrap();
    fs::write(fixture.join("Cargo.toml"),
        "[package]\nname = \"report-fixture\"\nversion = \"0.0.0\"\nedition = \"2021\"\n[workspace]\n").unwrap();
    fs::write(fixture.join("src/lib.rs"),
        "#[test] fn passing() {}\n#[test] fn failing() { panic!(\"intentional report negative control\"); }\n").unwrap();
    fs::write(
        fixture.join("src/main.rs"),
        "fn main() { eprintln!(\"diagnostic\"); println!(\"{{\\\"valid\\\":true}}\"); }\n",
    )
    .unwrap();
    // Runner intentionally uses this repository's resource-group profile.
    // Supply its named binary inventory so nextest validates the real config.
    fs::create_dir(fixture.join("tests")).unwrap();
    for binary in [
        "tooling",
        "schema_compiler",
        "api_contracts",
        "guard_quality",
        "memory_safety",
        "native_fuzz",
        "release",
        "runtime_simulation",
        "schedules",
    ] {
        fs::write(
            fixture.join(format!("tests/{binary}.rs")),
            "// Profile inventory fixture.\n",
        )
        .unwrap();
    }
    let command = |subcommand: &[&str]| {
        let mut command = v::command("cargo");
        command
            .current_dir(&fixture)
            .args(subcommand)
            .args(["--offline", "--manifest-path"])
            .arg(fixture.join("Cargo.toml"))
            .arg("--target-dir")
            .arg(directory.path().join("build"));
        command
    };
    let mut runner = Runner::new("report-test", &directory.path().join("evidence")).unwrap();
    let output = runner.directory.clone();
    assert!(!saved(&output).passed);
    assert_eq!(saved(&output).error.as_deref(), Some("run incomplete"));
    for (filter, passes) in [("test(=passing)", true), ("test(=failing)", false)] {
        let result = runner.nextest("tests", command(&["nextest", "run"]).args(["-E", filter]));
        assert_eq!(result.is_ok(), passes, "{result:?}");
        let report = fs::read(output.join("logs/tests.xml")).unwrap();
        let evidence = saved(&output);
        assert_eq!(
            evidence.data["test_reports"]["tests"]["sha256"],
            v::sha256(&report)
        );
        assert_eq!(evidence.steps.last().unwrap().passed, passes);
        let xml = String::from_utf8(report).unwrap();
        assert!(xml.contains(if passes {
            "name=\"passing\""
        } else {
            "name=\"failing\""
        }));
        assert_eq!(xml.contains("<failure"), !passes);
    }
    // A successful command that isn't nextest must not reuse the previous XML
    // or its digest. This exercises report provenance, not a synthetic test pass.
    let error = runner
        .nextest("tests", command(&["run", "--quiet"]).arg("--"))
        .unwrap_err();
    assert!(error.to_string().contains("no JUnit report"));
    assert!(!output.join("logs/tests.xml").exists());
    assert!(saved(&output).data["test_reports"].get("tests").is_none());
    assert!(runner.finish(Err(error)).is_err());
    assert!(!saved(&output).passed);

    let mut runner = Runner::new("report-test", &output).unwrap();
    assert!(saved(&output).steps.is_empty());
    let stdout = runner
        .run_stdout("json", &mut command(&["run", "--quiet"]))
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&stdout).unwrap()["valid"],
        true
    );
    assert!(fs::read_to_string(output.join("logs/json.log"))
        .unwrap()
        .contains("diagnostic"));
    runner.finish(Ok(())).unwrap();
    assert!(saved(&output).passed);
    runner.evidence.source_id = "different qualification source".into();
    assert!(runner
        .finish(Ok(()))
        .unwrap_err()
        .to_string()
        .contains("sources changed"));
    assert!(!saved(&output).passed);
}

#[test]
fn invalid_step_names_are_rejected_before_starting_a_command() {
    let directory = tempfile::tempdir().unwrap();
    let mut runner = Runner::new("names", directory.path()).unwrap();
    for name in ["", "../escape", "with space", "colon:invalid"] {
        for result in [
            runner.run(name, &mut Command::new("must-not-execute")),
            runner.nextest(name, &mut Command::new("must-not-execute")),
        ] {
            assert_eq!(result.unwrap_err().to_string(), "invalid log name");
        }
    }
    assert!(saved(directory.path()).steps.is_empty());
}
