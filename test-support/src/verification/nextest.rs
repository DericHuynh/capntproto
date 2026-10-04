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
