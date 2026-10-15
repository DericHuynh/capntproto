//! Pinned nextest machine output for nested verification campaigns.
//!
//! Aggregate CI counts use stable JUnit. These helpers only inspect individual
//! test events: nextest's experimental libtest suite counters are not libtest
//! aggregates and must never be added together.
use super::*;

pub fn json_output(cmd: &mut Command) -> &mut Command {
    cmd.env("NEXTEST_EXPERIMENTAL_LIBTEST_JSON", "1")
        .arg("--message-format=libtest-json-plus")
}

/// Reuse the current CI invocation's freshly compiled binaries for subprocess
/// replay. Never build into the output tree of an outer running nextest suite.
/// A missing requested binary fails closed rather than silently reducing cases.
pub fn replay_metadata(names: &[&str]) -> Result<Option<String>> {
    let Some(path) = std::env::var_os("CAPNTPROTO_TEST_INVENTORY") else {
        return Ok(None);
    };
    let bytes = fs::read(path)?;
    if sha256(&bytes) != std::env::var("CAPNTPROTO_TEST_INVENTORY_SHA256")? {
        return Err("nextest inventory changed during this invocation".into());
    }
    let inventory: serde_json::Value = serde_json::from_slice(&bytes)?;
    Ok(Some(select_replay_metadata(&inventory, names)?))
}

fn select_replay_metadata(inventory: &serde_json::Value, names: &[&str]) -> Result<String> {
    let suites = inventory["rust-suites"]
        .as_object()
        .ok_or("missing nextest suites")?;
    let mut binaries = BTreeMap::new();
    for name in names {
        let mut matches = suites
            .values()
            .filter(|s| s["package-name"] == "capntproto" && s["binary-name"] == *name);
        let suite = matches.next().ok_or("missing requested replay binary")?;
        if matches.next().is_some() || suite["status"] != "listed" {
            return Err("ambiguous or unlisted replay binary".into());
        }
        let mut binary = BTreeMap::new();
        for field in [
            "binary-id",
            "binary-name",
            "package-id",
            "kind",
            "binary-path",
            "build-platform",
        ] {
            let value = suite[field]
                .as_str()
                .ok_or("incomplete replay binary metadata")?;
            binary.insert(field, value);
        }
        if binaries.insert(binary["binary-id"], binary).is_some() {
            return Err("duplicate replay binary".into());
        }
    }
    if binaries.is_empty() || !inventory["rust-build-meta"].is_object() {
        return Err("empty replay metadata".into());
    }
    Ok(serde_json::to_string(&serde_json::json!({
        "rust-build-meta": inventory["rust-build-meta"], "rust-binaries": binaries,
    }))?)
}

/// Replayed instrumented binaries contribute to the same coverage campaign;
/// ordinary nested compiler commands still have instrumentation stripped.
pub fn replay_profiles(cmd: &mut Command) -> &mut Command {
    if std::env::var_os("CAPNTPROTO_TEST_INVENTORY").is_some() {
        if let Some(profiles) = std::env::var_os("CAPNTPROTO_FULL_COVERAGE_PROFILES") {
            cmd.env(
                "LLVM_PROFILE_FILE",
                PathBuf::from(profiles).join("%p-%m.profraw"),
            );
        }
    }
    cmd
}

pub fn outcomes(output: &str, event: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for row in output.lines().filter(|line| line.starts_with('{')) {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(row) else {
            continue;
        };
        if value["type"] == "test" && value["event"] == event {
            let name = value["name"].as_str().expect("test identity");
            let (_, name) = name.split_once('$').expect("nextest qualified test name");
            assert!(names.insert(name.to_owned()), "duplicate test name: {name}");
        }
    }
    names
}

pub fn passed(output: &str) -> BTreeSet<String> {
    assert!(outcomes(output, "failed").is_empty(), "{output}");
    let names = outcomes(output, "ok");
    assert!(
        !names.is_empty(),
        "nextest produced no passing test events: {output}"
    );
    names
}

pub fn inventory(output: &str, ignored_only: bool) -> BTreeSet<String> {
    let value: serde_json::Value = serde_json::from_str(output).expect("nextest list JSON");
    value["rust-suites"]
        .as_object()
        .expect("nextest suites")
        .values()
        .flat_map(|suite| suite["testcases"].as_object().expect("nextest test cases"))
        .filter(|(_, case)| {
            if ignored_only {
                case["ignored"] == true
            } else {
                case["filter-match"]["status"] == "matches"
            }
        })
        .map(|(name, _)| name.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replay_inventory_requires_each_exact_compiled_binary() {
        let suite = serde_json::json!({"package-name":"capntproto", "binary-name":"capntproto",
            "binary-id":"capntproto", "package-id":"owned", "kind":"lib", "status":"listed",
            "binary-path":"/build/owned-test", "build-platform":"target"});
        let mut inventory =
            serde_json::json!({"rust-suites":{"owned":suite}, "rust-build-meta":{}});
        let output: serde_json::Value =
            serde_json::from_str(&select_replay_metadata(&inventory, &["capntproto"]).unwrap())
                .unwrap();
        assert_eq!(
            output["rust-binaries"]["capntproto"]["binary-path"],
            "/build/owned-test"
        );
        assert!(select_replay_metadata(&inventory, &[]).is_err());
        assert!(select_replay_metadata(&inventory, &["authority_schedules"]).is_err());
        assert!(select_replay_metadata(&inventory, &["capntproto", "capntproto"]).is_err());
        inventory["rust-suites"]["duplicate"] = inventory["rust-suites"]["owned"].clone();
        assert!(select_replay_metadata(&inventory, &["capntproto"]).is_err());
        inventory["rust-suites"]
            .as_object_mut()
            .unwrap()
            .remove("duplicate");
        inventory["rust-suites"]["owned"]["status"] = "skipped".into();
        assert!(select_replay_metadata(&inventory, &["capntproto"]).is_err());
        inventory["rust-suites"]["owned"]["status"] = "listed".into();
        for field in [
            "binary-id",
            "binary-name",
            "package-id",
            "kind",
            "binary-path",
            "build-platform",
        ] {
            let mut malformed = inventory.clone();
            malformed["rust-suites"]["owned"]
                .as_object_mut()
                .unwrap()
                .remove(field);
            assert!(select_replay_metadata(&malformed, &["capntproto"]).is_err());
        }
        inventory["rust-build-meta"] = serde_json::Value::Null;
        assert!(select_replay_metadata(&inventory, &["capntproto"]).is_err());
        inventory["rust-suites"] = serde_json::Value::Null;
        assert!(select_replay_metadata(&inventory, &["capntproto"]).is_err());
    }

    #[test]
    fn nested_commands_do_not_inherit_temporary_nextest_profiles() {
        assert!(command("cargo")
            .get_envs()
            .any(|(key, value)| key == "NEXTEST_PROFILE" && value.is_none()));
    }

    #[test]
    fn individual_events_ignore_nextest_suite_counters_and_nested_stdout() {
        let output = concat!(
            "{\"type\":\"suite\",\"event\":\"started\",\"test_count\":3}\n",
            "{\"type\":\"test\",\"event\":\"ok\",\"name\":\"crate::binary$case\"}\n",
            "{\"type\":\"suite\",\"event\":\"ok\",\"passed\":99}\n",
            "{\"type\":\"suite\",\"event\":\"started\",\"test_count\":3}\n",
            "{\"type\":\"test\",\"event\":\"ignored\",\"name\":\"crate::binary$control\"}\n"
        );
        assert_eq!(passed(output), BTreeSet::from(["case".to_owned()]));
        assert_eq!(
            outcomes(output, "ignored"),
            BTreeSet::from(["control".to_owned()])
        );
    }

    #[test]
    fn inventory_distinguishes_ignored_and_filter_excluded_tests() {
        let output = r#"{"rust-suites":{"crate":{"testcases":{
            "selected":{"ignored":false,"filter-match":{"status":"matches"}},
            "filtered":{"ignored":false,"filter-match":{"status":"mismatch","reason":"expression"}},
            "control":{"ignored":true,"filter-match":{"status":"mismatch","reason":"ignored"}}
        }}}}"#;
        assert_eq!(
            inventory(output, false),
            BTreeSet::from(["selected".to_owned()])
        );
        assert_eq!(
            inventory(output, true),
            BTreeSet::from(["control".to_owned()])
        );
    }
}
