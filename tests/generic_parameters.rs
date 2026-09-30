use capnp::{capability::FromClientHook, data, text};
use reproto_test_support::runtime_test_capnp::{generic_outer::service, harness};
use std::rc::Rc;

// This signature fixes the public lexical parameter order independently of the
// generator. Three distinct types expose swapped aliases at compile time.
struct Service;
impl service::Server<data::Owned, text::Owned, harness::Owned> for Service {
    async fn exchange(
        self: Rc<Self>,
        params: service::ExchangeParams<data::Owned, harness::Owned>,
        mut results: service::ExchangeResults<data::Owned, text::Owned>,
    ) -> capnp::Result<()> {
        let params = params.get()?;
        let cap = params.get_first()?;
        let reply = cap.echo_request().send().promise.await?;
        assert_eq!(reply.get()?.get_value(), 73);
        results.get().set_first("text response")?;
        results.get().set_second(params.get_second()?)?;
        Ok(())
    }
    async fn repeat(
        self: Rc<Self>,
        params: service::RepeatParams<data::Owned, harness::Owned>,
        mut results: service::RepeatResults<data::Owned>,
    ) -> capnp::Result<()> {
        // The inherited A parameter is used by Pair's containing generic scope,
        // even where both explicit field arguments use C.
        let params = params.get()?;
        params.get_first()?.echo_request().send().promise.await?;
        params.get_second()?.echo_request().send().promise.await?;
        results.get().set_first(&b"one"[..])?;
        results.get().set_second(&b"two"[..])?;
        Ok(())
    }
}
struct Echo;
impl harness::Server for Echo {
    async fn echo(
        self: Rc<Self>,
        _: harness::EchoParams,
        mut results: harness::EchoResults,
    ) -> capnp::Result<()> {
        results.get().set_value(73);
        Ok(())
    }
}
#[tokio::test(flavor = "current_thread")]
async fn explicit_parameter_aliases_follow_lexical_order() {
    let client: service::Client<data::Owned, text::Owned, harness::Owned> =
        capnp_rpc::new_client(Service);
    let echo: harness::Client = capnp_rpc::new_client(Echo);
    let mut call = client.exchange_request();
    call.get().set_first(echo.clone()).unwrap();
    call.get().set_second(&b"data request"[..]).unwrap();
    let response = call.send().promise.await.unwrap();
    assert_eq!(
        response.get().unwrap().get_first().unwrap(),
        "text response"
    );
    assert_eq!(
        response.get().unwrap().get_second().unwrap(),
        b"data request"
    );
    let mut call = client.repeat_request();
    call.get().set_first(echo.clone()).unwrap();
    call.get().set_second(echo).unwrap();
    let response = call.send().promise.await.unwrap();
    assert_eq!(response.get().unwrap().get_first().unwrap(), b"one");
    assert_eq!(response.get().unwrap().get_second().unwrap(), b"two");
    let _ = client.into_client_hook();
}
#[test]
fn repeated_generator_runs_produce_identical_explicit_aliases() {
    let dir = tempfile::tempdir().unwrap();
    let mut previous = None;
    for _ in 0..16 {
        capnpc::CompilerCommand::new()
            .src_prefix("schemas")
            .file("schemas/runtime-test.capnp")
            .output_path(dir.path())
            .run()
            .unwrap();
        let generated = std::fs::read(dir.path().join("runtime_test_capnp.rs")).unwrap();
        if let Some(previous) = &previous {
            assert_eq!(previous, &generated);
        }
        previous = Some(generated);
    }
}

#[test]
fn replay_tlc_generic_parameter_cases() {
    let path = reproto_test_support::verification::input("REPROTO_GENERIC_PARAMETER_CASES")
        .expect("prepare verified trace corpus");
    let all_cases: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let cases: Vec<_> = all_cases.iter().filter(|c| c["group"] == false).collect();
    let groups: Vec<_> = all_cases.iter().filter(|c| c["group"] == true).collect();
    let dir = tempfile::tempdir().unwrap();
    let mut schema=String::from("@0xbafad976c5b10289;\nstruct Triple(X,Y,Z) {x @0 :X; y @1 :Y; z @2 :Z;}\nstruct Outer(A) { interface Service(B,C) {\n");
    let types = ["Data", "A", "B", "C"];
    for (i, case) in cases.iter().enumerate() {
        let args = case["args"]
            .as_array()
            .unwrap()
            .iter()
            .map(|n| types[n.as_u64().unwrap() as usize])
            .collect::<Vec<_>>()
            .join(",");
        schema.push_str(&format!("m{i} @{i} Triple({args}) -> Triple({args});\n"));
    }
    schema.push_str("}}\nstruct GroupScope(A) {struct Inner(B,C) {\n");
    for (i, case) in groups.iter().enumerate() {
        let args = case["args"].as_array().unwrap();
        schema.push_str(&format!("struct G{i} {{union {{payload :group {{ x @0 :{}; y @1 :{}; z @2 :{}; }} none @3 :Void; }} }}\n",
            types[args[0].as_u64().unwrap() as usize],types[args[1].as_u64().unwrap() as usize],types[args[2].as_u64().unwrap() as usize]));
    }
    schema.push_str("}}\n");
    let path = dir.path().join("brands.capnp");
    std::fs::write(&path, schema).unwrap();
    capnpc::CompilerCommand::new()
        .src_prefix(dir.path())
        .file(&path)
        .output_path(dir.path())
        .run()
        .unwrap();
    let generated = std::fs::read_to_string(dir.path().join("brands_capnp.rs")).unwrap();
    for (i, case) in groups.iter().enumerate() {
        assert_eq!(case["alias"], 123);
        let section = generated
            .split(&format!("pub mod g{i} {{"))
            .nth(1)
            .unwrap()
            .split("pub mod payload {")
            .next()
            .unwrap();
        for kind in ["Reader", "Builder"] {
            assert!(
                section.contains(&format!("pub type Which{kind}<'a,A,B,C> =")),
                "group {i}: {kind}"
            );
        }
    }
    for (i, case) in cases.iter().enumerate() {
        let alias = case["alias"].as_u64().unwrap();
        let params = if alias == 0 {
            String::new()
        } else {
            alias
                .to_string()
                .chars()
                .map(|n| format!("{},", types[n.to_digit(10).unwrap() as usize]))
                .collect::<String>()
        };
        for suffix in ["Params", "Results"] {
            let expected = format!("pub type M{i}{suffix}<{params}> =");
            assert!(generated.contains(&expected), "missing {expected}");
            let usage = format!("M{i}{suffix}<{params}>");
            assert_eq!(
                generated.matches(&usage).count(),
                2,
                "declaration and server argument {usage}"
            );
        }
    }
}

#[test]
fn group_union_aliases_bind_unused_enclosing_parameters() {
    use reproto_test_support::dynamic_test_capnp::group_scope::inner;
    let mut message = capnp::message::Builder::new_default();
    let mut root = message.init_root::<inner::Builder<data::Owned, text::Owned, harness::Owned>>();
    root.reborrow().init_payload().set_count(17);
    let branch: inner::WhichBuilder<'_, data::Owned, text::Owned, harness::Owned> =
        root.reborrow().which().unwrap();
    match branch {
        inner::Payload(mut g) => g.set_count(23),
        inner::Empty(()) => panic!("wrong union arm"),
    };
    let branch: inner::WhichReader<'_, data::Owned, text::Owned, harness::Owned> =
        root.into_reader().which().unwrap();
    match branch {
        inner::Payload(g) => assert_eq!(g.get_count(), 23),
        inner::Empty(()) => panic!("wrong union arm"),
    };
}
