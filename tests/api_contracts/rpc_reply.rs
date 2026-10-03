use super::{check_contracts, root};

#[test]
fn structured_rpc_reply_compile_contracts() {
    let prelude = "#![allow(dead_code)]\nuse capntproto_test_support::structured::rpc_api_capnp::{base, service};\n";
    let mut cases = vec![(
        "positive".into(),
        format!(
            "{prelude}{}",
            r#"
async fn echo(cap: &base::Client) -> capnp::Result<u32> {
    Ok(cap.echo_request().with_params(|mut p| { p.set_value(42); Ok(()) })?
        .send().await?.get()?.get_value())
}
fn immediate(reply: base::EchoResults) -> capnp::Result<()> {
    reply.complete(|mut value| { value.set_value(42); Ok(()) })
}
async fn forward(reply: service::ForwardResults, cap: base::Client) -> capnp::Result<()> {
    reply.tail_call(cap.echo_request()).await
}
async fn forward_cap(reply: service::ForwardCapResults, cap: service::Client) -> capnp::Result<()> {
    reply.tail_call(cap.open_request()).await
}
async fn published(reply: service::OpenResults, cap: base::Client) -> capnp::Result<()> {
    let mut draft = reply.build();
    draft.edit()?.set_cap(cap);
    let frozen = draft.publish()?;
    std::future::ready(()).await;
    frozen.finish()
}
async fn pipeline(cap: service::Client) -> capnp::Result<()> {
    let (response, pipeline) = cap.open_request().send().into_parts();
    let child = pipeline.get_cap().echo_request().send();
    child.await?;
    response.await?;
    Ok(())
}
fn main() {}
"#
        ),
        None,
    )];
    for (name, code, diagnostic) in [
        ("wrong-tail-results", "fn bad(r: service::OpenResults, c: base::Client) { drop(r.tail_call(c.echo_request())); }", "E0308"),
        ("streaming-tail", "fn bad(r: service::OpenResults, c: service::Client) { drop(r.tail_call(c.stream_request())); }", "E0308"),
        ("write-then-tail", "fn bad(r: service::OpenResults, c: service::Client) { let draft = r.build(); drop(draft.tail_call(c.open_request())); }", "E0599"),
        ("use-fresh-after-build", "fn bad(r: service::OpenResults, c: service::Client) { let _draft = r.build(); drop(r.tail_call(c.open_request())); }", "E0382"),
        ("edit-fresh-reply", "fn bad(mut r: service::OpenResults) { let _ = r.edit(); }", "E0599"),
        ("publish-twice", "fn bad(r: service::OpenResults) { let draft = r.build(); let _ = draft.publish(); let _ = draft.publish(); }", "E0382"),
        ("republish-frozen", "fn bad(r: service::OpenResults) { let frozen = r.build().publish().unwrap(); let _ = frozen.publish(); }", "E0599"),
        ("edit-frozen", "fn bad(r: service::OpenResults) { let mut frozen = r.build().publish().unwrap(); let _ = frozen.edit(); }", "E0599"),
        ("forward-frozen", "fn bad(r: service::OpenResults, c: service::Client) { let frozen = r.build().publish().unwrap(); drop(frozen.tail_call(c.open_request())); }", "E0599"),
        ("publish-with-live-editor", "fn bad(r: service::OpenResults) { let mut draft = r.build(); let mut view = draft.edit().unwrap(); let _ = draft.publish(); let _ = view.reborrow(); }", "E0505"),
        ("escape-reply-editor", "fn bad(r: service::OpenResults) -> service::opened::Builder<'static> { let mut draft = r.build(); draft.edit().unwrap() }", "E0515"),
        ("raw-hook-private", "fn bad(r: service::OpenResults) { let _ = r.hook; }", "E0616"),
        ("builder-hook-private", "fn bad(r: service::OpenResults) { let _ = r.build().hook; }", "E0616"),
        ("forge-default", "fn main() { let _: service::OpenResults = Default::default(); }", "E0277"),
        ("clone-reply", "fn bad(r: service::OpenResults) { let _ = r.clone(); }", "E0599"),
        ("finish-twice", "fn bad(r: service::OpenResults) { let draft = r.build(); let _ = draft.finish(); let _ = draft.finish(); }", "E0382"),
        ("send-twice", "fn bad(c: base::Client) { let call = c.echo_request(); drop(call.send()); drop(call.send()); }", "E0382"),
        ("escape-parameter-editor", "fn bad(c: base::Client) { let mut escaped = None; let _ = c.echo_request().with_params(|p| { escaped = Some(p); Ok(()) }); }", "E0521"),
        ("reply-thread-affinity", "fn require<T: Send>() {} fn main() { require::<service::OpenResults>(); }", "E0277"),
    ] {
        let main = if code.contains("fn main()") { "" } else { "fn main() {}" };
        cases.push((name.into(), format!("{prelude}{code}\n{main}"), Some(diagnostic)));
    }
    cases.push(("request-must-use".into(), format!("#![deny(unused_must_use)]\n{prelude}fn bad(c: base::Client) {{ c.echo_request(); }}\nfn main() {{}}"), Some("unused_must_use")));
    cases.push(("published-owner-must-use".into(), format!("#![deny(unused_must_use)]\n{prelude}fn bad(r: service::OpenResults) {{ r.build().publish().unwrap(); }}\nfn main() {{}}"), Some("unused_must_use")));
    check_contracts("rpc-reply-contracts", &format!(
        "capnp = {{ package = \"capntproto-core\", path = {:?} }}\ncapntproto-test-support = {{ path = {:?} }}\n",
        root().join("crates/capntproto-core"), root().join("test-support")
    ), cases,
    "external consumers: affine reply stages, exact tail result schema, editor lifetimes, consuming requests and generated legacy/structured client compatibility",
    "ordinary generated API only; private runtime hook implementors remain trusted; Rust permits explicit drops/forget and ignored Result values; runtime network/application failures remain possible");
}
