#![cfg(target_os = "linux")]
mod common;
use capnp::{capability::FromClientHook, Result};
use capnp_compat::{
    byte_stream::{ByteStreamFactory, OutputStream},
    byte_stream_capnp::byte_stream,
    http::{self, ConnectResponseSender, ResponseSender, WebSocketSink},
    websocket::Message,
};
use futures::{future::LocalBoxFuture, FutureExt};
use reproto_test_support::verification::{command, cpp, root, run};
use std::{
    cell::{Cell, RefCell},
    fs,
    process::Stdio,
    rc::Rc,
};
use tokio::io::AsyncBufReadExt;
use tokio_util::compat::TokioAsyncReadCompatExt;
#[derive(Default)]
struct Capture {
    bytes: RefCell<Vec<u8>>,
    ended: Cell<bool>,
    aborted: Cell<bool>,
}
impl OutputStream for Capture {
    fn write<'a>(&'a self, b: &'a [u8]) -> LocalBoxFuture<'a, Result<()>> {
        async move {
            self.bytes.borrow_mut().extend(b);
            Ok(())
        }
        .boxed_local()
    }
    fn end(&self) -> LocalBoxFuture<'_, Result<()>> {
        async {
            self.ended.set(true);
            Ok(())
        }
        .boxed_local()
    }
    fn abort(&self) {
        self.aborted.set(true);
    }
}
struct Response {
    body: Rc<Capture>,
    socket: Rc<Socket>,
    up: RefCell<Option<Rc<dyn WebSocketSink>>>,
}
impl ResponseSender for Response {
    fn start_response(
        &self,
        r: http::Response,
    ) -> LocalBoxFuture<'_, Result<Rc<dyn OutputStream>>> {
        async move {
            assert_eq!(r.status_code, 200);
            Ok(self.body.clone() as Rc<dyn OutputStream>)
        }
        .boxed_local()
    }
    fn start_websocket(
        &self,
        _: http::Headers,
        up: Rc<dyn WebSocketSink>,
    ) -> LocalBoxFuture<'_, Result<Rc<dyn WebSocketSink>>> {
        async move {
            *self.up.borrow_mut() = Some(up);
            Ok(self.socket.clone() as Rc<dyn WebSocketSink>)
        }
        .boxed_local()
    }
}
#[derive(Default)]
struct Socket {
    messages: RefCell<Vec<Message>>,
}
impl WebSocketSink for Socket {
    fn send(&self, m: Message) -> LocalBoxFuture<'_, Result<()>> {
        async move {
            self.messages.borrow_mut().push(m);
            Ok(())
        }
        .boxed_local()
    }
}
struct ConnectResponse;
impl ConnectResponseSender for ConnectResponse {
    fn start_connect(&self, r: http::Response) -> LocalBoxFuture<'_, Result<()>> {
        async move {
            assert_eq!(r.status_code, 200);
            Ok(())
        }
        .boxed_local()
    }
    fn start_error(&self, _: http::Response) -> LocalBoxFuture<'_, Result<Rc<dyn OutputStream>>> {
        async { panic!("unexpected CONNECT error") }.boxed_local()
    }
}
#[tokio::test(flavor = "current_thread")]
async fn rust_adapters_interoperate_with_pinned_cpp_factories() {
    let build = cpp::build(&[
        "capnp-rpc",
        "kj-http",
        "capnp_tool",
        "capnpc_cpp",
        "capnpc",
        "capnp-json",
    ])
    .unwrap();
    let logs = root().join("target/verification/compat");
    fs::create_dir_all(&logs).unwrap();
    let binary = logs.join("adapters");
    let generated = logs.join("generated");
    fs::create_dir_all(&generated).unwrap();
    run(
        command(build.join("c++/src/capnp/capnp").to_str().unwrap())
            .args([
                "compile",
                "-Ivendor/capnproto/c++/src",
                "--src-prefix=vendor/capnproto/c++/src",
            ])
            .arg(format!(
                "-o{}:{}",
                build.join("c++/src/capnp/capnpc-c++").display(),
                generated.display()
            ))
            .args([
                "vendor/capnproto/c++/src/capnp/compat/byte-stream.capnp",
                "vendor/capnproto/c++/src/capnp/compat/http-over-capnp.capnp",
                "vendor/capnproto/c++/src/capnp/compat/json-rpc.capnp",
            ]),
        &logs.join("generate.log"),
        0,
    )
    .unwrap();
    let mut compile = command("g++");
    compile.arg(format!("-I{}", generated.display()));
    compile.args([
        "-std=c++23",
        "-Ivendor/capnproto/c++/src",
        "crates/capnp-compat/tests/cpp-adapters.c++",
        "vendor/capnproto/c++/src/capnp/compat/byte-stream.c++",
        "vendor/capnproto/c++/src/capnp/compat/http-over-capnp.c++",
        "vendor/capnproto/c++/src/capnp/compat/json-rpc.c++",
    ]);
    compile
        .arg(generated.join("capnp/compat/byte-stream.capnp.c++"))
        .arg(generated.join("capnp/compat/http-over-capnp.capnp.c++"));
    compile.arg(generated.join("capnp/compat/json-rpc.capnp.c++"));
    for lib in [
        "capnp/libcapnp-rpc.a",
        "capnp/libcapnp-json.a",
        "capnp/libcapnpc.a",
        "capnp/libcapnp.a",
        "kj/libkj-http.a",
        "kj/libkj-async.a",
        "kj/libkj.a",
    ] {
        compile.arg(build.join("c++/src").join(lib));
    }
    compile.args(["-pthread", "-o"]).arg(&binary);
    run(&mut compile, &logs.join("adapters-build.log"), 0).unwrap();
    tokio::time::timeout(
        std::time::Duration::from_secs(30),
        tokio::task::LocalSet::new().run_until(async {
            for mode in ["byte", "http", "json"] {
                let output = logs.join(format!("{mode}.bytes"));
                let mut process = tokio::process::Command::from(command(binary.to_str().unwrap()));
                process
                    .args([mode, output.to_str().unwrap()])
                    .stdout(Stdio::piped())
                    .stderr(fs::File::create(logs.join(format!("{mode}-server.log"))).unwrap())
                    .kill_on_drop(true);
                let mut child = process.spawn().unwrap();
                let mut stdout = tokio::io::BufReader::new(child.stdout.take().unwrap());
                let mut line = String::new();
                stdout.read_line(&mut line).await.unwrap();
                let port = line.trim().parse::<u16>().expect("C++ loopback listener");
                let stream = tokio::net::TcpStream::connect(("127.0.0.1", port))
                    .await
                    .unwrap();
                stream.set_nodelay(true).unwrap();
                let (read, write) = futures::AsyncReadExt::split(stream.compat());
                if mode == "json" {
                    use capnp::schema_loader::dynamic::{ServiceSchema, Value};
                    use capnp_compat::json_rpc::{ContentLengthTransport, JsonRpc};
                    let parsed = common::schemas();
                    let schema = ServiceSchema::new(
                        common::loader(),
                        common::schema(&parsed, "Calculator").id(),
                    )
                    .unwrap();
                    let (json, driver) =
                        JsonRpc::new(Rc::new(ContentLengthTransport::new(read, write)), None)
                            .unwrap();
                    let driver = tokio::task::spawn_local(driver);
                    {
                        let client = schema
                            .reflect(json.get_peer(schema.clone()).unwrap())
                            .unwrap();
                        let mut request = client.new_request("add").unwrap();
                        request
                            .get()
                            .unwrap()
                            .set_named("x", Value::Int32(21))
                            .unwrap();
                        request
                            .get()
                            .unwrap()
                            .set_named("y", Value::Int32(21))
                            .unwrap();
                        let response = request.send().unwrap().resolve().await.unwrap();
                        assert!(matches!(
                            response.get().unwrap().get_named("value").unwrap(),
                            Value::Int32(42)
                        ));
                        client
                            .new_request("notify")
                            .unwrap()
                            .send()
                            .unwrap()
                            .resolve()
                            .await
                            .unwrap();
                        let error = client
                            .new_request("fail")
                            .unwrap()
                            .send()
                            .unwrap()
                            .resolve()
                            .await
                            .err()
                            .unwrap();
                        assert!(error.to_string().contains("expected C++ failure"));
                    }
                    driver.abort();
                    let _ = driver.await;
                    drop(json);
                    assert!(child.wait().await.unwrap().success());
                    continue;
                }
                let network = capnp_rpc::twoparty::VatNetwork::new(
                    read,
                    write,
                    capnp_rpc::rpc_twoparty_capnp::Side::Client,
                    Default::default(),
                );
                let mut rpc = capnp_rpc::RpcSystem::new(Box::new(network), None);
                let bootstrap: capnp::capability::Client =
                    rpc.bootstrap(capnp_rpc::rpc_twoparty_capnp::Side::Server);
                let disconnect = rpc.get_disconnector();
                let driver = tokio::task::spawn_local(rpc);
                if mode == "byte" {
                    let client = byte_stream::Client::new(bootstrap.into_client_hook());
                    let factory = ByteStreamFactory::new();
                    let output = factory.to_output(client);
                    output.write(b"standard C++ ByteStream").await.unwrap();
                    output.end().await.unwrap();
                    assert_eq!(
                        fs::read(logs.join("byte.bytes")).unwrap(),
                        b"standard C++ ByteStream"
                    );
                } else {
                    let client = http::HttpOverCapnpFactory::default().to_service(
                        capnp_compat::http_over_capnp_capnp::http_service::Client::new(
                            bootstrap.into_client_hook(),
                        ),
                    );
                    let response = Rc::new(Response {
                        body: Rc::new(Capture::default()),
                        socket: Rc::new(Socket::default()),
                        up: RefCell::new(None),
                    });
                    let bytes = vec![123; 200_000];
                    client
                        .request(
                            http::Request {
                                method: http::HttpMethod::Post,
                                url: "/echo".into(),
                                headers: vec![(
                                    "Content-Type".into(),
                                    "application/octet-stream".into(),
                                )],
                                body_size: http::BodySize::Fixed(bytes.len() as u64),
                            },
                            Box::new(futures::io::Cursor::new(bytes.clone())),
                            response.clone(),
                        )
                        .await
                        .unwrap();
                    assert_eq!(*response.body.bytes.borrow(), bytes);
                    assert!(response.body.ended.get());
                    assert!(!response.body.aborted.get());
                    let request = client.request(
                        http::Request {
                            method: http::HttpMethod::Get,
                            url: "/ws".into(),
                            headers: vec![],
                            body_size: http::BodySize::Fixed(0),
                        },
                        Box::new(futures::io::Cursor::new(vec![])),
                        response.clone(),
                    );
                    let send = async {
                        loop {
                            let up = response.up.borrow().clone();
                            if let Some(up) = up {
                                up.send(Message::Binary(b"websocket".to_vec()))
                                    .await
                                    .unwrap();
                                break;
                            }
                            tokio::task::yield_now().await;
                        }
                    };
                    let (result, ()) = futures::join!(request, send);
                    result.unwrap();
                    assert_eq!(
                        *response.socket.messages.borrow(),
                        vec![
                            Message::Binary(b"websocket".to_vec()),
                            Message::Close {
                                code: Some(1000),
                                reason: "done".into()
                            }
                        ]
                    );
                    let down = Rc::new(Capture::default());
                    let connection = client
                        .connect(
                            http::ConnectRequest {
                                host: "example.test:443".into(),
                                headers: vec![],
                                use_tls: false,
                            },
                            down.clone(),
                            Rc::new(ConnectResponse),
                        )
                        .await
                        .unwrap();
                    let pump = async {
                        connection.up.write(b"CONNECT tunnel").await.unwrap();
                        connection.up.end().await.unwrap();
                    };
                    let (result, ()) = futures::join!(connection.completion, pump);
                    result.unwrap();
                    assert_eq!(&*down.bytes.borrow(), b"CONNECT tunnel");
                    assert!(down.ended.get());
                }
                disconnect.await.unwrap();
                let _ = driver.await;
                let status = child.wait().await.unwrap();
                assert!(status.success(), "{mode} C++ server: {status}");
            }
        }),
    )
    .await
    .expect("C++ interop deadline");
}
