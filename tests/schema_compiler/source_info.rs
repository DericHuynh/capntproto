use super::*;

#[test]
fn documentation_and_positions_match_pinned_cpp() {
    let compiler = cpp::build(&["capnp_tool"])
        .unwrap()
        .join("c++/src/capnp/capnp");
    let directory = tempfile::tempdir().unwrap();
    let mut cases =
        vec![include_str!("../../crates/capnp-compiler/examples/documentation.capnp").to_owned()];
    for prefix in ["", "\n", "\r\n", "\u{feff}", "# prelude\n"] {
        for doc in [
            "",
            " # doc 🦀\n",
            "\n# doc\n#  second\n#\n",
            "\n\n# ordinary\n",
            "\r# CR-only comment\n",
            "\r\n# CRLF\r\n",
            "\t#\tindented\n",
            "\n\u{feff}# BOM stops attachment\n",
        ] {
            cases.push(format!("{prefix}@0xabcdefabcdefabcd;{doc}struct S {{{doc} a @1 :Text;{doc} b @0 :Void;{doc}}}{doc}enum E {{ e @0;{doc}}}{doc}interface I {{ f @0 (p :Text, # ordinary parameter comment\n n :UInt32 = 7) -> (value :S);{doc} }}{doc}"));
        }
    }
    for suffix in [
        "#",
        "# trailing",
        "\n# trailing",
        "\r\n# final 🦀",
        "\n\n# unattached",
        "\n# first\n\n# unattached",
        "\n# first\n \t# second",
        "\n# first\n \n# unattached",
    ] {
        cases.push(format!("@0xabcdefabcdefabcd; struct S {{}}{suffix}"));
    }
    cases.push("@0xabcdefabcdefabcd; struct S { # opening\n} # ignored closing\n".into());
    cases.push("@0xabcdefabcdefabcd; enum E {} interface I {} struct S {}".into());
    cases.push("@0xabcdefabcdefabcd; const text :Text = `# literal { } ;\n; # real doc\n".into());
    for source in &cases {
        fs::write(directory.path().join("docs.capnp"), source).unwrap();
        let reference = command(&compiler)
            .current_dir(directory.path())
            .args(["compile", "-o-", "docs.capnp"])
            .output()
            .unwrap();
        assert!(
            reference.status.success(),
            "{source}\n{}",
            String::from_utf8_lossy(&reference.stderr)
        );
        let reference = capnp::serialize::read_message(
            reference.stdout.as_slice(),
            message::ReaderOptions::new(),
        )
        .unwrap();
        let rust = capnp_compiler::compile("docs.capnp", source).unwrap();
        eprintln!("source-info case: {source:?}");
        compare_requests(
            rust.get_root_as_reader().unwrap(),
            reference.get_root().unwrap(),
        );
    }
    fs::write(root().join("target/verification/schema-compiler/source-info.txt"), format!("{} accepted documentation requests match pinned C++, including node/member positions and comments\n", cases.len())).unwrap();
}
