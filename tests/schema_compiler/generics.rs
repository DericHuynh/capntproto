use super::*;

#[test]
fn generic_types_and_methods_match_pinned_cpp() {
    let compiler = cpp::build(&["capnp_tool"])
        .unwrap()
        .join("c++/src/capnp/capnp");
    let directory = tempfile::tempdir().unwrap();
    let mut cases: Vec<String> = [
        "struct G(T) { value @0 :T(); }",
        "interface I { f @0 [T] (value :T()); }",
        "struct Box(T) { value @0 :T; next @1 :Box; } struct Use { a @0 :Box; b @1 :Box(Text); }",
        "struct Outer(T) { struct Inner(U) { t @0 :T; u @1 :U; outer @2 :Outer; self @3 :Inner; } } struct S { x @0 :Outer(Text).Inner(Data); y @1 :Outer.Inner; }",
        "struct Box(T) { value @0 :T; again @1 :Box(T); }",
        "struct Outer(T) { struct Inner { t @0 :T; } inner @0 :Inner; }",
        "struct Outer(T) { enum E { a @0; b @1; } e @0 :E = b; } struct S { e @0 :Outer(Text).E = a; }",
        "struct Outer(T) { value :group { t @0 :T; union { a @1 :Void; b @2 :T; } } }",
        "struct Box() {} struct S { value @0 :Box(); }",
        "struct Box(T,T) { value @0 :T; }",
        "struct Box(t, a_b) { value @0 :t; }",
        "struct Box(T) { struct T {} value @0 :T; }",
        "struct Box(T) { value @0 :T; } using B = Box; struct S { value @0 :B(Data); }",
        "struct Box(T) { value @0 :T; } using B = Box(Text); struct S { value @0 :B; }",
        "struct Outer(T) { using Alias = T; struct Inner(U) { t @0 :Alias; u @1 :U; } } struct S { value @0 :Outer(Text).Inner(Data); }",
        "struct Box(T) { value @0 :T; } struct Outer(T) { using B = Box(T); field @0 :B; } struct S { f @0 :Outer(Text).B; }",
        "struct Box(T) { value @0 :T; } struct Outer(T) { using B = Box; } struct S { f @0 :Outer(Text).B(Data); }",
        "struct Box(T) { value @0 :T; } struct S { f @0 :List(Box(Text)); }",
        "struct Box(T) { value @0 :T; } const x :Box(Text) = (value = \"yes\"); struct S { f @0 :Box(Text) = .x; }",
        "struct Box(T) { value @0 :T; } const x :Box(List(Text)) = (value = [\"yes\", \"no\"]);",
        "struct Box(T) { value @0 :T; } const x :List(Box(Text)) = [(value=\"one\"), (value=\"two\")];",
        "struct Box(T) { value @0 :T; } struct S { f @0 :Box(Text) = \"wrapped\"; }",
        "struct Box(T) { value @0 :T; } const x :Box = (); struct S { f @0 :Box(AnyPointer) = .x; }",
        "struct Box(T) { value @0 :T; const x :Text = \"yes\"; } const x :Text = Box(Data).x;",
        "struct Box(T) { value @0 :T; annotation a(struct) :T; } struct S $Box(Text).a(\"hi\") {}",
        "struct Outer(T) { annotation a(struct) :Void; struct S $a {} } struct X $Outer(Data).a {}",
        "struct Outer(T) { struct Inner(U) { annotation a(*) :Void; } } $Outer(Text).Inner(Data).a;",
        "interface I(T) { f @0 (t :T) -> (t :T); } struct S { i @0 :I(Text); }",
        "interface Base(T) { f @0 (x :T); } interface Derived(U) extends(Base(U)) { g @0 (x :U); }",
        "struct S {} interface I extends(Base(S)) {} interface Base(T) {}",
        "interface I(T) { f @0 [U] (t :T, u :U) -> (u :U); g @1 (t :T); }",
        "struct Box(T) { value @0 :T; } interface I(T) { f @0 [U] Box(U) -> Box(T); }",
        "struct Box(T) { value @0 :T; } interface I { f @0 [U] (x :Box(U)); }",
        "interface I { f @0 [T, T] (x :T = null); }",
        "interface I { f @0 [t] (x :t); }",
        "interface I(T) { f @0 [T] (x :T) -> (x :T); }",
        "interface I { f @0 [] (); }",
        "interface I { f @0 (t :Text = null, d :Data = null, a :AnyPointer = null, c :Capability = null, i :I = null, l :List(UInt32) = null) -> (t :Text = null); }",
        "struct S { a @0 :AnyStruct; l @1 :AnyList; ls @2 :List(AnyList); }",
        "struct S { n @0 :UInt32; } const s :S = (n=4); const x :AnyStruct = .s; const l :List(UInt32) = [1, 2]; const y :AnyList = .l;",
    ].into_iter().map(str::to_owned).collect();
    cases.extend([
        "struct G(T) { annotation a(param) :Void; } interface I(T) { f @0 (x :T $G(T).a); }",
        "struct G(T) { value @0 :T; } interface I(T) { f @0 [U] (x :G(U) = ()); }",
        "struct B(T) { value @0 :T; } struct G(T) { const v :B(T) = (); } const x :B(Text) = G(Text).v; const y :B(Data) = G(Data).v;",
        "struct G(T) {} struct X { f @0 :G(G); }",
        "struct S { f @0 :Text(); }",
        "struct G(T) { using T = Text; value @0 :T; }",
        "struct G(A,B) { annotation a(*) :Void; } $((G(Text,Data)).a);",
    ].map(str::to_owned));
    cases.push(
        include_str!("../../crates/capntproto-compiler/examples/generics.capnp")
            .split_once(';')
            .unwrap()
            .1
            .to_owned(),
    );
    for ty in [
        "Text",
        "Data",
        "AnyPointer",
        "List(UInt32)",
        "List(List(Text))",
        "S",
        "I",
        "Box(Text)",
    ] {
        cases.push(format!("struct S {{}} interface I {{}} struct Box(T) {{ value @0 :T; }} struct Use {{ value @0 :Box({ty}); }}"));
    }
    for body in &cases {
        let source = format!("@0xabcdefabcdefabcd; {body}");
        fs::write(directory.path().join("test.capnp"), &source).unwrap();
        let reference = command(&compiler)
            .current_dir(directory.path())
            .args(["compile", "-o-", "test.capnp"])
            .output()
            .unwrap();
        assert!(
            reference.status.success(),
            "{body}\n{}",
            String::from_utf8_lossy(&reference.stderr)
        );
        let reference = capnp::serialize::read_message(
            reference.stdout.as_slice(),
            message::ReaderOptions::new(),
        )
        .unwrap();
        let rust = capntproto_compiler::compile("test.capnp", &source)
            .unwrap_or_else(|e| panic!("{body}: {e}"));
        eprintln!("generic case: {body}");
        compare_requests(
            rust.get_root_as_reader().unwrap(),
            reference.get_root().unwrap(),
        );
    }
    let invalid = [
        "struct G(T) { annotation a(param) :Void; } interface I { f @0 [U] (x :U $G(U).a); }",
        "struct G(T) { value @0 :T; } struct S { value @0 :G(Text).T; }",
        "enum E(T) { a @0; }",
        "struct S(3) {}",
        "struct S(T) { f @0 :U; }",
        "struct S(T) { f @0 :T = 1; }",
        "struct S(T) { f @0 :List(T); }",
        "struct S(T) {} struct X { f @0 :S(); }",
        "struct S(T) {} struct X { f @0 :S(Text, Data); }",
        "struct S {} struct X { f @0 :S(Text); }",
        "struct X { f @0 :Text(Text); }",
        "struct S(T) {} struct X { f @0 :S(UInt32); }",
        "struct S(T) {} struct X { f @0 :S(Capability); }",
        "struct S(T) {} struct X { f @0 :S(AnyStruct); }",
        "struct S(T) {} struct X { f @0 :S(AnyList); }",
        "struct S(T) {} enum E { a @0; } struct X { f @0 :S(E); }",
        "struct S(T) {} using B = S(Text); struct X { f @0 :B(Data); }",
        "struct S(T) { f @0 :T; } const x :S(Text) = (f=\"x\"); const y :S(Data) = .x;",
        "struct S(T) { f @0 :T; } const x :S = (f=\"x\");",
        "interface I(T) extends(T) {}",
        "interface I { f @0 [T] T; }",
        "interface I { f @0 [T] () -> T; }",
        "interface I { f @0 [T] (x :List(T)); }",
        "interface I { f @0 [T] (x :T = \"x\"); }",
        "interface I { f @0 (x :UInt32 = null); }",
        "struct S { f @0 :Text = null; }",
        "struct S { f @0 :List(AnyStruct); }",
        "struct S(T) { annotation a(struct) :T; } struct X $S(UInt32).a(1) {}",
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
            capntproto_compiler::compile("test.capnp", &source).is_err(),
            "Rust accepted {body}"
        );
    }
    fs::write(root().join("target/verification/schema-compiler/generics.txt"), format!("reference: 0de72d8d8cec6b69edaa29de51d3bd490341f9c2\n{} accepted generic schemas match all known request fields and canonical annotation/default/constant bytes\n{} shared rejections\n", cases.len(), invalid.len())).unwrap();
}

#[test]
fn generic_imports_and_standard_schemas_match_pinned_cpp() {
    let compiler = cpp::build(&["capnp_tool"])
        .unwrap()
        .join("c++/src/capnp/capnp");
    let directory = tempfile::tempdir().unwrap();
    let sources = [
        (
            "types.capnp",
            r#"@0xbbbbbbbbbbbbbbbb;
            struct Box(T) { value @0 :T; struct Nested(U) { outer @0 :T; inner @1 :U; } annotation mark(*) :T; }
            interface Base(T) { f @0 (x :T) -> (y :T); }
            struct Payload { text @0 :Text; }
            const value :Box(Text) = (value = "imported");
            interface Unused(T) { bad @0 (x :import "missing.capnp".Bad); }
        "#,
        ),
        (
            "middle.capnp",
            r#"@0xcccccccccccccccc; using Box = import "types.capnp".Box;
            using Bound = Box(Text); using Base = import "types.capnp".Base;
            struct Outer(T) { using B = Box(T); using Generic = Box; using Item = T; }
        "#,
        ),
        (
            "cycle.capnp",
            r#"@0xdddddddddddddddd; struct Node(T) { next @0 :import "main.capnp".Node(T); value @1 :T; }"#,
        ),
    ];
    for (name, source) in sources {
        fs::write(directory.path().join(name), source).unwrap();
    }
    let cases = [
        "using T = import \"types.capnp\"; struct S { value @0 :T.Box(Text); }",
        "using M = import \"middle.capnp\"; struct S { value @0 :M.Box(Data); bound @1 :M.Bound; }",
        "using M = import \"middle.capnp\"; struct S { value @0 :M.Outer(Text).B; item @1 :M.Outer(Data).Item; generic @2 :M.Outer(Text).Generic(Data); }",
        "using T = import \"types.capnp\"; struct S { value @0 :T.Box(Text).Nested(Data); }",
        "using T = import \"types.capnp\"; struct S(U) { value @0 :T.Box(U).Nested(T.Payload); }",
        "using T = import \"types.capnp\"; interface I(U) extends(T.Base(U)) { f @0 (v :T.Box(U)) -> (v :T.Box(U)); }",
        "using T = import \"types.capnp\"; interface I { f @0 [U] T.Box(U) -> T.Box(Text); }",
        "struct S { value @0 :import \"middle.capnp\".Box(import \"types.capnp\".Payload); }",
        "const value :AnyPointer = import \"types.capnp\".value;",
        "using T = import \"types.capnp\"; const value :T.Box(Text) = T.value;",
        "using T = import \"types.capnp\"; struct S $T.Box(Text).mark(\"tag\") {}",
        "struct S $import \"types.capnp\".Box(import \"types.capnp\".Payload).mark(text=\"tag\") {}",
        "using T = import \"types.capnp\"; struct S $(T.Box(Text).mark)(\"tag\") {}",
        "using T = import \"types.capnp\"; struct S $((T.Box(Text)).mark)(\"tag\") {}",
        "struct Node(T) { next @0 :import \"cycle.capnp\".Node(T); value @1 :T; }",
    ];
    let mut frontend = capntproto_compiler::FileCompiler::new();
    frontend.src_prefix(directory.path());
    for source in cases {
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
        eprintln!("generic import: {source}");
        compare_requests(
            rust.get_root_as_reader().unwrap(),
            reference.get_root().unwrap(),
        );
    }
    for names in [
        ["main.capnp", "middle.capnp"],
        ["middle.capnp", "main.capnp"],
    ] {
        let reference = command(&compiler)
            .current_dir(directory.path())
            .args(["compile", "-o-"])
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
    let standard = root().join("vendor/capnproto/c++/src");
    let fixtures = [
        "capnp/persistent.capnp",
        "capnp/rpc.capnp",
        "capnp/rpc-twoparty.capnp",
        "capnp/schema.capnp",
    ];
    for name in fixtures {
        let reference = command(&compiler)
            .current_dir(&standard)
            .args(["compile", "-o-", "-I.", name])
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
        let rust = capntproto_compiler::FileCompiler::new()
            .src_prefix(&standard)
            .import_path(&standard)
            .compile(&[standard.join(name)])
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        eprintln!("standard generic: {name}");
        compare_requests(
            rust.get_root_as_reader().unwrap(),
            reference.get_root().unwrap(),
        );
    }
    fs::write(root().join("target/verification/schema-compiler/generic-imports.txt"), format!("reference: 0de72d8d8cec6b69edaa29de51d3bd490341f9c2\n{} generic import graphs and {} unmodified standard schemas match all known request fields and canonical annotation/default/constant bytes\n", cases.len()+2, fixtures.len())).unwrap();
}

#[test]
fn rust_generics_generate_working_structs_and_rpc_bindings() {
    let project = tempfile::tempdir().unwrap();
    fs::create_dir(project.path().join("src")).unwrap();
    let request = capntproto_compiler::compile(
        "generics.capnp",
        include_str!("../../crates/capntproto-compiler/examples/generics.capnp"),
    )
    .unwrap();
    capnpc::codegen::CodeGenerationCommand::new()
        .output_directory(project.path().join("src"))
        .run(capnp::serialize::write_message_to_words(&request).as_slice())
        .unwrap();
    fs::write(project.path().join("Cargo.toml"), format!(
        "[package]\nname = \"rust-generic-acceptance\"\nversion = \"0.0.0\"\nedition = \"2021\"\n[workspace]\n[dependencies]\ncapnp = {{ package = \"capntproto-core\", path = {:?} }}\ncapnp-rpc = {{ package = \"capntproto-rpc\", path = {:?} }}\ntokio = {{ version = \"1\", features = [\"rt\", \"macros\", \"time\"] }}\n", root().join("crates/capntproto-core"), root().join("crates/capntproto-rpc"))).unwrap();
    fs::write(project.path().join("src/lib.rs"), r#"
pub mod generics_capnp { include!("generics_capnp.rs"); }
#[cfg(test)]
mod tests {
    use super::generics_capnp::{defaults, envelope, echo, service, outer};
    use capnp::{text, data, any_pointer, capability::FromClientHook};
    use std::rc::Rc;
    struct Server;
    impl echo::Server<text::Owned> for Server {
        async fn echo(self: Rc<Self>, params: echo::EchoParams<text::Owned>, mut results: echo::EchoResults<text::Owned>) -> capnp::Result<()> {
            results.get().set_value(params.get()?.get_value()?)?;
            Ok(())
        }
    }
    impl service::Server<text::Owned> for Server {
        async fn wrap(self: Rc<Self>, params: service::WrapParams<text::Owned>, mut results: service::WrapResults<text::Owned>) -> capnp::Result<()> {
            results.get().init_value().set_value(params.get()?.get_value()?)?;
            Ok(())
        }
        async fn relay(self: Rc<Self>, params: service::RelayParams<text::Owned>, mut results: service::RelayResults<text::Owned>) -> capnp::Result<()> {
            results.get().get_value()?.set_as::<any_pointer::Owned>(params.get()?.get_value()?)?;
            Ok(())
        }
    }
    impl outer::service::Server<data::Owned, text::Owned, envelope::Owned<text::Owned>> for Server {
        async fn exchange(self: Rc<Self>, params: outer::service::ExchangeParams<data::Owned, envelope::Owned<text::Owned>>, mut results: outer::service::ExchangeResults<data::Owned, text::Owned>) -> capnp::Result<()> {
            let params = params.get()?;
            results.get().set_first(params.get_first()?.get_value()?)?;
            results.get().set_second(params.get_second()?)?;
            Ok(())
        }
    }
    #[test]
    fn defaults_groups_brands_and_serialization() {
        let mut message = capnp::message::Builder::new_default();
        let mut root = message.init_root::<defaults::Builder>();
        assert_eq!(root.reborrow().get_message().unwrap().get_value().unwrap(), "generic default");
        let mut value = root.reborrow().get_message().unwrap();
        value.set_value("changed").unwrap();
        value.get_metadata().set_backup("backup").unwrap();
        let bytes = capnp::serialize::write_message_to_words(&message);
        let decoded = capnp::serialize::read_message(bytes.as_slice(), Default::default()).unwrap();
        let value = decoded.get_root::<defaults::Reader>().unwrap().get_message().unwrap();
        assert_eq!(value.get_value().unwrap(), "changed");
        assert_eq!(value.get_metadata().get_backup().unwrap(), "backup");
        assert_eq!(value.get_metadata().get_state().unwrap(), envelope::State::Ready);
    }
    #[tokio::test(flavor = "current_thread")]
    async fn inherited_generic_methods_and_explicit_nested_signatures() {
        tokio::task::LocalSet::new().run_until(async {
            tokio::time::timeout(std::time::Duration::from_secs(10), async {
                let (executor, driver) = capnp_rpc::new_call_executor();
                let task = tokio::task::spawn_local(driver);
                let client: service::Client<text::Owned> = capnp_rpc::new_client_with_executor(Server, executor.clone());
                let base = echo::Client::<text::Owned>::new(client.clone().into_client_hook());
                let mut request = base.echo_request();
                request.get().set_value("inherited generic").unwrap();
                assert_eq!(request.send().promise.await.unwrap().get().unwrap().get_value().unwrap(), "inherited generic");
                let mut request = client.wrap_request();
                request.get().set_value("wrapped").unwrap();
                assert_eq!(request.send().promise.await.unwrap().get().unwrap().get_value().unwrap().get_value().unwrap(), "wrapped");
                let mut request = client.relay_request();
                request.get().get_value().unwrap().set_as::<text::Owned>("implicit method parameter").unwrap();
                assert_eq!(request.send().promise.await.unwrap().get().unwrap().get_value().unwrap().get_as::<text::Reader>().unwrap(), "implicit method parameter");
                let nested: outer::service::Client<data::Owned, text::Owned, envelope::Owned<text::Owned>> = capnp_rpc::new_client_with_executor(Server, executor);
                let mut request = nested.exchange_request();
                request.get().init_first().set_value("nested").unwrap();
                request.get().set_second(b"bytes".as_slice()).unwrap();
                let response = request.send().promise.await.unwrap();
                assert_eq!(response.get().unwrap().get_first().unwrap(), "nested");
                assert_eq!(response.get().unwrap().get_second().unwrap(), b"bytes");
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
        &root().join("target/verification/schema-compiler/generics-rust.log"),
        0,
    )
    .unwrap();
}

#[test]
fn repository_schemas_match_pinned_cpp_with_rust_frontend() {
    let compiler = cpp::build(&["capnp_tool"])
        .unwrap()
        .join("c++/src/capnp/capnp");
    let schemas = root().join("schemas");
    let standard = root().join("vendor/capnproto/c++/src");
    let rust_imports = root().join("crates/capntproto-codegen");
    let mut files: Vec<_> = fs::read_dir(&schemas)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "capnp"))
        .collect();
    files.sort();
    let mut frontend = capntproto_compiler::FileCompiler::new();
    frontend
        .src_prefix(&schemas)
        .import_path(&standard)
        .import_path(&rust_imports);
    for path in &files {
        let name = path.file_name().unwrap();
        let reference = command(&compiler)
            .current_dir(&schemas)
            .args(["compile", "-o-", "-I"])
            .arg(&standard)
            .arg("-I")
            .arg(&rust_imports)
            .arg(name)
            .output()
            .unwrap();
        assert!(
            reference.status.success(),
            "{}: {}",
            path.display(),
            String::from_utf8_lossy(&reference.stderr)
        );
        let reference = capnp::serialize::read_message(
            reference.stdout.as_slice(),
            message::ReaderOptions::new(),
        )
        .unwrap();
        let rust = frontend
            .compile(&[path])
            .unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        eprintln!("repository schema: {}", path.display());
        compare_requests(
            rust.get_root_as_reader().unwrap(),
            reference.get_root().unwrap(),
        );
    }
    fs::write(root().join("target/verification/schema-compiler/repository-schemas.txt"), format!("reference: 0de72d8d8cec6b69edaa29de51d3bd490341f9c2\n{} unmodified repository schemas match all known request fields and canonical annotation/default/constant bytes\n", files.len())).unwrap();
}

#[test]
fn unbound_list_elements_are_rejected_consistently_across_parameter_indices() {
    let compiler = cpp::build(&["capnp_tool"])
        .unwrap()
        .join("c++/src/capnp/capnp");
    let directory = tempfile::tempdir().unwrap();
    // Pinned C++ reads the parameter index as an unconstrained-pointer tag:
    // it rejects A/B but accepts C. Rust keeps the unsupported type diagnostic
    // independent of parameter order instead of reproducing this accident.
    for (parameter, accepted) in [("A", false), ("B", false), ("C", true)] {
        let source =
            format!("@0xabcdefabcdefabcd; struct G(A,B,C) {{ value @0 :List({parameter}); }}");
        fs::write(directory.path().join("test.capnp"), &source).unwrap();
        let reference = command(&compiler)
            .current_dir(directory.path())
            .args(["compile", "-o-", "test.capnp"])
            .output()
            .unwrap();
        assert_eq!(reference.status.success(), accepted);
        let error = capntproto_compiler::compile("test.capnp", &source)
            .err()
            .unwrap();
        assert!(error.message.contains("unbound parameters"), "{error}");
    }
}
