use super::*;

#[path = "../../crates/capntproto-compiler/tests/corpus/grammar.rs"]
mod corpus;
#[path = "../../crates/capntproto-compiler/tests/corpus/numbers.rs"]
mod numbers;

#[test]
fn numeric_tokens_in_requested_and_dependency_only_files_match_pinned_cpp() {
    let cases = [(numbers::VALID, true), (numbers::INVALID, false)];
    check_cases(
        cases
            .into_iter()
            .flat_map(|(numbers, accepted)| {
                numbers
                    .iter()
                    .enumerate()
                    .map(move |(index, number)| corpus::Case {
                        name: format!("number-{accepted}-{index}"),
                        body: format!("const value :Float64 = {number};"),
                        accepted,
                    })
            })
            .collect(),
        "numeric",
    );

    let build = cpp::build(&["capnp_tool"]).unwrap();
    let compiler = build.join("c++/src/capnp/capnp");
    let directory = tempfile::tempdir().unwrap();
    let logs = root().join("target/verification/schema-compiler/numeric-imports");
    fs::create_dir_all(&logs).unwrap();
    let main =
        "@0xaaaaaaaaaaaaaaaa; using D = import \"types.capnp\"; struct Root { value @0 :D.Used; }";
    fs::write(directory.path().join("main.capnp"), main).unwrap();
    for (numbers, accepted) in cases {
        for (index, number) in numbers.iter().enumerate() {
            let source =
                format!("@0xbbbbbbbbbbbbbbbb; struct Used {{}} const hidden :Float64 = {number};");
            fs::write(directory.path().join("types.capnp"), &source).unwrap();
            let output = command(&compiler)
                .current_dir(directory.path())
                .args(["compile", "-o-", "main.capnp"])
                .output()
                .unwrap();
            let mut parser = capntproto_compiler::SchemaParser::new();
            parser.add_source("main.capnp", main).unwrap();
            parser.add_source("types.capnp", &source).unwrap();
            let rust = parser.parse(&["main.capnp"]);
            fs::write(logs.join(format!("{accepted}-{index}.capnp")), source).unwrap();
            fs::write(
                logs.join(format!("{accepted}-{index}.cpp.txt")),
                &output.stderr,
            )
            .unwrap();
            assert_eq!(
                output.status.success(),
                accepted,
                "{number}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(
                rust.is_ok(),
                accepted,
                "{number}: {:?}",
                rust.as_ref().err()
            );
            if accepted {
                let reference = capnp::serialize::read_message(
                    output.stdout.as_slice(),
                    message::ReaderOptions::new(),
                )
                .unwrap();
                compare_requests(
                    rust.unwrap().get_root_as_reader().unwrap(),
                    reference.get_root().unwrap(),
                );
            } else {
                assert_eq!(output.status.code(), Some(1), "{number}");
                assert_eq!(rust.err().unwrap().filename, "types.capnp");
            }
        }
    }
    fs::write(logs.join("summary.txt"), format!("{} dependency-only imports match known request fields and canonical values/source-info; {} shared lexical rejections in unused declarations. Diagnostic wording and C++ compiler version excluded.\n", numbers::VALID.len(), numbers::INVALID.len())).unwrap();
}

#[test]
fn contextual_names_and_numeric_grammar_match_pinned_cpp() {
    check_cases(corpus::cases(), "grammar");
}

#[test]
fn control_bytes_and_whitespace_match_pinned_cpp() {
    check_cases(corpus::lexical_cases(), "lexical");
}

fn check_cases(cases: Vec<corpus::Case>, name: &str) {
    let build = cpp::build(&["capnp_tool"]).unwrap();
    let compiler = build.join("c++/src/capnp/capnp");
    let directory = tempfile::tempdir().unwrap();
    let logs = root()
        .join("target/verification/schema-compiler")
        .join(name);
    fs::create_dir_all(&logs).unwrap();
    let mut accepted = 0;
    let mut rejected = 0;
    let mut mismatches = Vec::new();
    for case in cases {
        let source = case.source();
        fs::write(directory.path().join("grammar.capnp"), &source).unwrap();
        let output = command(&compiler)
            .current_dir(directory.path())
            .args(["compile", "-o-", "grammar.capnp"])
            .output()
            .unwrap();
        let rust = capntproto_compiler::compile("grammar.capnp", &source);
        fs::write(logs.join(format!("{}.capnp", case.name)), &source).unwrap();
        fs::write(logs.join(format!("{}.cpp.txt", case.name)), &output.stderr).unwrap();
        assert_eq!(
            output.status.success(),
            case.accepted,
            "{}: {}",
            case.name,
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            rust.is_ok(),
            case.accepted,
            "{}: {:?}",
            case.name,
            rust.as_ref().err()
        );
        if case.accepted {
            accepted += 1;
            let reference = capnp::serialize::read_message(
                output.stdout.as_slice(),
                message::ReaderOptions::new(),
            )
            .unwrap();
            let reference = reference
                .get_root::<code_generator_request::Reader<'_>>()
                .unwrap();
            let rust = rust.unwrap();
            let rust = rust
                .get_root_as_reader::<code_generator_request::Reader<'_>>()
                .unwrap();
            fs::write(
                logs.join(format!("{}.cpp-request.txt", case.name)),
                format!("{reference:?}"),
            )
            .unwrap();
            fs::write(
                logs.join(format!("{}.rust-request.txt", case.name)),
                format!("{rust:?}"),
            )
            .unwrap();
            // Retain every mismatching case from this bounded corpus in one run.
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                compare_requests(rust, reference)
            }))
            .is_err()
            {
                mismatches.push(case.name);
            }
        } else {
            rejected += 1;
            assert_eq!(output.status.code(), Some(1), "{}", case.name);
            assert!(!output.stderr.is_empty(), "{}", case.name);
            fs::write(
                logs.join(format!("{}.rust.txt", case.name)),
                rust.err().unwrap().to_string(),
            )
            .unwrap();
        }
    }
    assert!(mismatches.is_empty(), "request mismatches: {mismatches:?}");
    fs::write(logs.join("summary.txt"), format!("{accepted} accepted schemas match known request fields, canonical values/source-info and normalized identifier references; {rejected} shared rejections. C++ compiler version and diagnostic wording excluded.\n")).unwrap();
}
