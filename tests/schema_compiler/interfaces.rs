use super::*;

#[test]
fn interfaces_match_pinned_cpp() {
    let build = cpp::build(&["capnp_tool"]).unwrap();
    let compiler = build.join("c++/src/capnp/capnp");
    let directory = tempfile::tempdir().unwrap();
    let mut cases: Vec<String> = [
        "interface I {}",
        "interface I extends() {}",
        "interface I { f @0 (X :Bool, x_y :Text, struct :UInt32); }",
        "interface I { f @0 (); }",
        "interface I { f @0 () -> (); }",
        "interface I { second @1 () -> (value :Text); first @0 (value :UInt32); }",
        "interface I @0xdeadbeefdeadbeef { f @0 (a :Bool, b :UInt64, c :UInt8, d :UInt16) -> (s :Text,); }",
        "struct P { x @0 :UInt32; } interface I { f @0 P -> P; g @1 P; h @2 () -> P; }",
        "interface I { struct P { x @0 :UInt32; } f @0 P -> P; }",
        "struct Outer { interface Inner { f @0 (x :Outer) -> (i :Inner); } i @0 :Inner; }",
        "interface I { interface Inner { f @0 (x :I) -> (i :Inner); } f @0 (x :Inner); }",
        "interface Base { ping @0 (); } interface I extends(Base) { ping @0 (); }",
        "interface A {} interface B {} interface I extends(A, B,) {}",
        "interface A {} interface I extends(A, A) {}",
        "interface I extends(I) {}",
        "interface A extends(B) {} interface B extends(A) {}",
        "interface A {} interface B extends(A) {} interface C extends(A) {} interface I extends(B, C) {}",
        "interface I extends(Base) { interface Base {} }",
        "interface Base {} using B = .Base; interface I extends(B) {}",
        "interface I { using T = UInt32; const d :UInt32 = 42; f @0 (x :T = I.d) -> (x :T = I.d); }",
        "interface I { enum E { a @0; b @1; } f @0 (x :E = b) -> (y :E = a); }",
        "interface I { struct P { text @0 :Text; n @1 :UInt16 = 9; } const d :P = (text = \"yes\"); f @0 (x :P = I.d) -> (y :List(P) = [(), I.d]); }",
        "annotation a(interface) :Text; annotation b(method) :Text; annotation c(param) :Text; interface I $a(\"i\") { f @0 (x :Bool $c(\"x\")) -> (y :Text $c(\"y\")) $b(\"f\"); }",
        "annotation a(*) :UInt16; interface I $a(1) { last @1 (x :UInt32 $a(2)) $a(3); first @0 (x :Bool $a(4)) -> (x :Text $a(5)) $a(6); }",
        "interface I $a { annotation a(interface, method, param) :Void; f @0 (x :Bool $a) -> (y :Bool $a) $a; }",
        "interface I {} struct S { i @0 :I; c @1 :Capability; is @2 :List(I); cs @3 :List(Capability); }",
        "interface I {} const empty :List(I) = [];",
        "interface I { f @0 (i :I, c :Capability, is :List(I), cs :List(Capability)); }",
        "interface I { struct P {} using Params = P; f @0 Params -> .I.Params; }",
    ].into_iter().map(str::to_owned).collect();
    for (ty, value) in [
        ("Void", "void"),
        ("Bool", "true"),
        ("Int8", "-128"),
        ("Int16", "32767"),
        ("Int32", "-2147483648"),
        ("Int64", "-9223372036854775808"),
        ("UInt8", "255"),
        ("UInt16", "65535"),
        ("UInt32", "4294967295"),
        ("UInt64", "18446744073709551615"),
        ("Float32", "0.1"),
        ("Float64", "-0.0"),
        ("Text", "\"hé🦀\""),
        ("Data", "0x\"00ff\""),
        ("List(Bool)", "[true, false]"),
        ("List(Text)", "[\"\", \"x\"]"),
        ("List(List(UInt32))", "[[1, 2], []]"),
    ] {
        cases.push(format!(
            "interface I {{ f @0 (value :{ty} = {value}) -> (value :{ty} = {value}); }}"
        ));
    }
    for count in [1, 2, 3, 8, 17, 64] {
        let methods: String = (0..count)
            .rev()
            .map(|i| format!("m{i} @{i} (n :UInt32 = {i}) -> (next :I); "))
            .collect();
        cases.push(format!("interface I {{ {methods} }}"));
    }
    for (index, body) in cases.iter().enumerate() {
        let source = format!("@0xabcdefabcdefabcd; {body}");
        fs::write(directory.path().join("test.capnp"), &source).unwrap();
        let reference = command(&compiler)
            .current_dir(directory.path())
            .args(["compile", "-o-", "test.capnp"])
            .output()
            .unwrap();
        assert!(
            reference.status.success(),
            "case {index}: {source}\n{}",
            String::from_utf8_lossy(&reference.stderr)
        );
        let reference = capnp::serialize::read_message(
            reference.stdout.as_slice(),
            message::ReaderOptions::new(),
        )
        .unwrap();
        let rust = capnp_compiler::compile("test.capnp", &source)
            .unwrap_or_else(|e| panic!("case {index}: {source}\n{e}"));
        if [
            "interface I extends(I) {}",
            "interface A extends(B) {} interface B extends(A) {}",
        ]
        .contains(&body.as_str())
        {
            let rust = rust.get_root_as_reader().unwrap();
            let reference = reference.get_root().unwrap();
            assert!(SchemaLoader::default().load_request(rust).is_err());
            assert!(SchemaLoader::default().load_request(reference).is_err());
            compare_request_fields(rust, reference);
        } else {
            compare_requests(
                rust.get_root_as_reader().unwrap(),
                reference.get_root().unwrap(),
            );
        }
    }
    let invalid = [
        "interface i {}",
        "interface Bad_Name {}",
        "interface I @5 {}",
        "interface I { f @1 (); }",
        "interface I { f @0 (); g @0 (); }",
        "interface I { f @0 (); f @1 (); }",
        "interface I { F @0 (); }",
        "interface I { f @65536 (); }",
        "interface I { f (); }",
        "interface I { f @0; }",
        "interface I { f @0 () ->; }",
        "interface I { f @0 (x :Bool x :Text); }",
        "interface I { f @0 (x @0 :Bool); }",
        "interface I { f @0 (x :Bool, x :Text); }",
        "interface I { f @0 () -> (x :Bool, x :Text); }",
        "interface I { f @0 (x :UInt8 = 256); }",
        "interface I { f @0 (x :Bool) -> (x :UInt8 = 256); }",
        "interface I { f @0 UInt32; }",
        "interface I { f @0 () -> I; }",
        "interface I { f @0 Missing; }",
        "struct P {} interface I { f @0 (P); }",
        "struct S {} interface I extends(S) {}",
        "interface I extends(Capability) {}",
        "interface I extends(Missing) {}",
        "interface I { f @0 (); const f :Void = void; }",
        "interface I { const f :Void = void; f @0 (); }",
        "interface I { using f = .x; f @0 (); } const x :Void = void;",
        "interface A { struct P {} } interface I extends(A) { f @0 P; }",
        "interface A { struct P {} } interface I extends(A) { f @0 I.P; }",
        "annotation a(field) :Void; interface I { f @0 (x :Bool $a); }",
        "annotation a(field) :Void; interface I { f @0 () -> (x :Bool $a); }",
        "annotation a(param) :Void; interface I { f @0 () $a; }",
        "annotation a(struct) :Void; interface I $a {}",
        "interface I {} struct S { f @0 :I = void; }",
        "struct S { f @0 :Capability = void; }",
        "interface I {} const c :I = void;",
        "interface I {} const c :List(I) = [void];",
        "interface I { f @0 stream; }",
    ];
    for body in invalid {
        let source = format!("@0xabcdefabcdefabcd; {body}");
        fs::write(directory.path().join("test.capnp"), &source).unwrap();
        let reference = command(&compiler)
            .current_dir(directory.path())
            .args(["compile", "-o-", "test.capnp"])
            .output()
            .unwrap();
        assert!(!reference.status.success(), "C++ accepted {body}");
        assert!(
            capnp_compiler::compile("test.capnp", &source).is_err(),
            "Rust accepted {body}"
        );
    }
    fs::write(root().join("target/verification/schema-compiler/interfaces.txt"), format!(
        "reference: 0de72d8d8cec6b69edaa29de51d3bd490341f9c2\n{} accepted interface schemas match all known request fields and canonical method/param annotation and default bytes\n{} shared rejections\n", cases.len(), invalid.len())).unwrap();
}

#[test]
fn imported_interfaces_and_streaming_match_pinned_cpp() {
    let build = cpp::build(&["capnp_tool"]).unwrap();
    let compiler = build.join("c++/src/capnp/capnp");
    let directory = tempfile::tempdir().unwrap();
    let standard = root().join("vendor/capnproto/c++/src");
    for (name, source) in [
        (
            "base.capnp",
            r#"@0xbbbbbbbbbbbbbbbb;
            interface Base { ping @0 (n :UInt32 = 42) -> (text :Text); struct Params { n @0 :UInt32; } }
            interface Other { call @0 () -> (base :Base); }
            annotation label(interface, method, param) :Text;
            interface Unused { bad @0 (n :import "missing.capnp".Type); }
        "#,
        ),
        (
            "mid.capnp",
            r#"@0xcccccccccccccccc; using Base = import "base.capnp".Base;
            using Other = import "base.capnp".Other; using Params = Base.Params;
            using label = import "base.capnp".label;
            interface Derived extends(Base, Other) { own @0 (base :Base) -> (d :Derived); }
        "#,
        ),
        (
            "cycle.capnp",
            r#"@0xdddddddddddddddd;
            interface Back { call @0 (main :import "main.capnp".I); }
        "#,
        ),
    ] {
        fs::write(directory.path().join(name), source).unwrap();
    }
    let cases = [
        "interface I extends(import \"base.capnp\".Base) {}",
        "using B = import \"base.capnp\"; interface I extends(B.Base, B.Other) { f @0 B.Base.Params -> B.Base.Params; }",
        "using M = import \"mid.capnp\"; interface I extends(M.Derived) { f @0 M.Params -> (base :M.Base); }",
        "using M = import \"mid.capnp\"; interface I $M.label(\"interface\") { f @0 (base :M.Base $M.label(\"param\")) -> (other :M.Other $M.label(\"result\")) $M.label(\"method\"); }",
        "interface I { f @0 (other :import \"cycle.capnp\".Back) -> (i :I); }",
        "interface I { f @0 () -> stream; }",
        "interface I { f @0 (bytes :Data) -> stream; g @1 () -> stream; }",
        "using M = import \"mid.capnp\"; interface I extends(M.Base) { f @0 M.Params -> stream; }",
        "interface I { f @0 () -> import \"/capnp/stream.capnp\".StreamResult; }",
        "using S = import \"/capnp/stream.capnp\"; interface I { f @0 () -> stream; g @1 () -> S.StreamResult; }",
    ];
    let mut frontend = capnp_compiler::FileCompiler::new();
    frontend.src_prefix(directory.path()).import_path(&standard);
    for source in cases {
        fs::write(
            directory.path().join("main.capnp"),
            format!("@0xabcdefabcdefabcd; {source}"),
        )
        .unwrap();
        let reference = command(&compiler)
            .current_dir(directory.path())
            .args(["compile", "-o-", "-I"])
            .arg(&standard)
            .arg("main.capnp")
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
        let rust = frontend
            .compile(&[directory.path().join("main.capnp")])
            .unwrap_or_else(|e| panic!("{source}: {e}"));
        compare_requests(
            rust.get_root_as_reader().unwrap(),
            reference.get_root().unwrap(),
        );
    }
    // A second requested file rebases its auxiliary method nodes in the graph.
    for names in [["main.capnp", "mid.capnp"], ["mid.capnp", "main.capnp"]] {
        let reference = command(&compiler)
            .current_dir(directory.path())
            .args(["compile", "-o-", "-I"])
            .arg(&standard)
            .args(names)
            .output()
            .unwrap();
        assert!(
            reference.status.success(),
            "{}",
            String::from_utf8_lossy(&reference.stderr)
        );
        let reference = capnp::serialize::read_message(
            reference.stdout.as_slice(),
            message::ReaderOptions::new(),
        )
        .unwrap();
        let rust = frontend
            .compile(&names.map(|name| directory.path().join(name)))
            .unwrap();
        compare_requests(
            rust.get_root_as_reader().unwrap(),
            reference.get_root().unwrap(),
        );
    }
    let invalid = [
        "interface I extends(import \"base.capnp\".Base.Params) {}",
        "interface I extends(import \"base.capnp\".Unused) {}",
        "interface I { f @0 import \"mid.capnp\".Base; }",
        "interface I { f @0 () -> import \"mid.capnp\".Other; }",
        "interface I { f @0 (x :import \"base.capnp\".Missing); }",
        "using M = import \"mid.capnp\"; interface I { f @0 () $M.label(7); }",
    ];
    for source in invalid {
        fs::write(
            directory.path().join("main.capnp"),
            format!("@0xabcdefabcdefabcd; {source}"),
        )
        .unwrap();
        let reference = command(&compiler)
            .current_dir(directory.path())
            .args(["compile", "-o-", "main.capnp"])
            .output()
            .unwrap();
        assert!(!reference.status.success(), "C++ accepted {source}");
        assert!(
            frontend
                .compile(&[directory.path().join("main.capnp")])
                .is_err(),
            "Rust accepted {source}"
        );
    }
    fs::write(root().join("target/verification/schema-compiler/interface-imports.txt"), format!(
        "reference: 0de72d8d8cec6b69edaa29de51d3bd490341f9c2\n{} accepted interface import/streaming graphs match all known request fields and canonical method/param annotation and default bytes\n{} shared rejections\n", cases.len() + 2, invalid.len())).unwrap();
}

#[test]
fn rust_interfaces_generate_working_rpc_bindings() {
    let project = tempfile::tempdir().unwrap();
    fs::create_dir(project.path().join("src")).unwrap();
    let mut parser = capnp_compiler::SchemaParser::new();
    parser
        .add_source(
            "interfaces.capnp",
            include_str!("../../crates/capnp-compiler/examples/interfaces.capnp"),
        )
        .unwrap();
    parser
        .add_source(
            "capnp/stream.capnp",
            include_str!("../../vendor/capnproto/c++/src/capnp/stream.capnp"),
        )
        .unwrap();
    parser
        .add_source(
            "capnp/c++.capnp",
            include_str!("../../vendor/capnproto/c++/src/capnp/c++.capnp"),
        )
        .unwrap();
    parser.import_path(".").unwrap();
    let request = parser.parse(&["interfaces.capnp"]).unwrap();
    capnpc::codegen::CodeGenerationCommand::new()
        .output_directory(project.path().join("src"))
        .run(capnp::serialize::write_message_to_words(&request).as_slice())
        .unwrap();
    fs::write(project.path().join("Cargo.toml"), format!(
        "[package]\nname = \"rust-interface-acceptance\"\nversion = \"0.0.0\"\nedition = \"2021\"\n[workspace]\n[dependencies]\ncapnp = {{ path = {:?} }}\ncapnp-rpc = {{ path = {:?} }}\ntokio = {{ version = \"1\", features = [\"rt\", \"macros\", \"time\"] }}\n", root().join("vendor/capnp"), root().join("vendor/capnp-rpc"))).unwrap();
    fs::write(project.path().join("src/lib.rs"), r#"
pub mod interfaces_capnp { include!("interfaces_capnp.rs"); }
#[cfg(test)]
mod tests {
    use super::interfaces_capnp::{base, service};
    use std::{cell::RefCell, rc::Rc};
    struct Server(Rc<RefCell<Vec<u8>>>, Rc<dyn capnp::capability::CallExecutor>);
    impl base::Server for Server {
        async fn ping(self: Rc<Self>, params: base::PingParams, mut results: base::PingResults) -> capnp::Result<()> {
            results.get().set_value(params.get()?.get_value() + 1);
            Ok(())
        }
    }
    impl service::Server for Server {
        async fn echo(self: Rc<Self>, params: service::EchoParams, mut results: service::EchoResults) -> capnp::Result<()> {
            let params = params.get()?;
            let mut results = results.get();
            results.set_text(params.get_text()?);
            results.set_count(params.get_count() * 2);
            Ok(())
        }
        async fn get_peer(self: Rc<Self>, _: service::GetPeerParams, mut results: service::GetPeerResults) -> capnp::Result<()> {
            results.get().set_peer(capnp_rpc::new_client_with_executor(Server(self.0.clone(), self.1.clone()), self.1.clone()));
            Ok(())
        }
        async fn upload(self: Rc<Self>, params: service::UploadParams) -> capnp::Result<()> {
            self.0.borrow_mut().extend_from_slice(params.get()?.get_bytes()?);
            Ok(())
        }
    }
    #[tokio::test(flavor = "current_thread")]
    async fn inherited_explicit_streaming_and_pipelined_calls() {
        tokio::task::LocalSet::new().run_until(async {
            tokio::time::timeout(std::time::Duration::from_secs(10), async {
                let bytes = Rc::new(RefCell::new(vec![]));
                let (executor, driver) = capnp_rpc::new_call_executor();
                let task = tokio::task::spawn_local(driver);
                let client: service::Client = capnp_rpc::new_client_with_executor(Server(bytes.clone(), executor.clone()), executor);
                use capnp::capability::FromClientHook;
                let parent = base::Client::new(client.clone().into_client_hook());
                let response = parent.ping_request().send().promise.await.unwrap();
                assert_eq!(response.get().unwrap().get_value(), 43);
                let response = client.echo_request().send().promise.await.unwrap();
                assert_eq!(response.get().unwrap().get_text().unwrap(), "default");
                assert_eq!(response.get().unwrap().get_count(), 14);
                let mut request = client.echo_request();
                request.get().set_text("RPC from Rust source");
                request.get().set_count(11);
                let response = request.send().promise.await.unwrap();
                assert_eq!(response.get().unwrap().get_text().unwrap(), "RPC from Rust source");
                assert_eq!(response.get().unwrap().get_count(), 22);
                let peer = client.get_peer_request().send();
                let mut ping = peer.pipeline.get_peer().ping_request();
                ping.get().set_value(99);
                assert_eq!(ping.send().promise.await.unwrap().get().unwrap().get_value(), 100);
                peer.promise.await.unwrap();
                for chunk in [b"one".as_slice(), b"two".as_slice()] {
                    let mut upload = client.upload_request();
                    upload.get().set_bytes(chunk);
                    upload.send().await.unwrap();
                }
                assert_eq!(&*bytes.borrow(), b"onetwo");
                let schema = service::Client::schema();
                assert_eq!(schema.get_methods().unwrap().len(), 3);
                assert_eq!(schema.get_superclasses().unwrap().len(), 1);
                task.abort();
            }).await.unwrap();
        }).await;
    }
}
"#).unwrap();
    run(
        command("cargo")
            .args(["test", "--offline", "--manifest-path"])
            .arg(project.path().join("Cargo.toml"))
            .env(
                "CARGO_TARGET_DIR",
                root().join("target/schema-compiler-acceptance"),
            ),
        &root().join("target/verification/schema-compiler/interfaces-rust.log"),
        0,
    )
    .unwrap();
}

#[test]
fn cyclic_deep_and_exponential_inheritance_fail_codegen_without_recursing_forever() {
    let directory = tempfile::tempdir().unwrap();
    for body in [
        "interface I extends(I) {}",
        "interface A extends(B) {} interface B extends(A) {}",
    ] {
        let request =
            capnp_compiler::compile("cycle.capnp", &format!("@0xabcdefabcdefabcd; {body}"))
                .unwrap();
        let error = capnpc::codegen::CodeGenerationCommand::new()
            .output_directory(directory.path())
            .run(capnp::serialize::write_message_to_words(&request).as_slice())
            .unwrap_err();
        assert!(error.to_string().contains("cyclic interface"), "{error}");
    }
    for (count, duplicated, expected) in
        [(70, false, "nesting limit"), (18, true, "expansion limit")]
    {
        let mut parser = capnp_compiler::SchemaParser::new();
        let mut source = "@0xbbbbbbbbbbbbbbbb; interface I0 {}".to_owned();
        for i in 1..count {
            let bases = if duplicated {
                format!("I{}, I{}", i - 1, i - 1)
            } else {
                format!("I{}", i - 1)
            };
            source.push_str(&format!("interface I{i} extends({bases}) {{}}"));
        }
        parser.add_source("base.capnp", source).unwrap();
        parser
            .add_source(
                "test.capnp",
                format!(
                    "@0xabcdefabcdefabcd; interface I extends(import \"base.capnp\".I{}) {{}}",
                    count - 1
                ),
            )
            .unwrap();
        let request = parser.parse(&["test.capnp"]).unwrap();
        let error = capnpc::codegen::CodeGenerationCommand::new()
            .output_directory(directory.path())
            .run(capnp::serialize::write_message_to_words(&request).as_slice())
            .unwrap_err();
        assert!(error.to_string().contains(expected), "{error}");
    }
}
