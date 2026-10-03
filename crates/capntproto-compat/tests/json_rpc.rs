mod common;
use capnp::{
    capability::{Client, FromClientHook, Promise},
    schema_loader::dynamic::{self, ServiceSchema, Value},
};
use capntproto_compat::json_rpc::{ContentLengthTransport, Endpoint, JsonRpc, Transport};
use futures::{future::LocalBoxFuture, task::LocalSpawnExt, FutureExt, StreamExt};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
struct Channel {
    input: futures::lock::Mutex<futures::channel::mpsc::UnboundedReceiver<String>>,
    output: futures::channel::mpsc::UnboundedSender<String>,
}
impl Transport for Channel {
    fn send<'a>(&'a self, s: &'a str) -> LocalBoxFuture<'a, capnp::Result<()>> {
        async move {
            self.output
                .unbounded_send(s.into())
                .map_err(|_| capnp::Error::disconnected("closed".into()))
        }
        .boxed_local()
    }
    fn receive(&self) -> LocalBoxFuture<'_, capnp::Result<String>> {
        async {
            self.input
                .lock()
                .await
                .next()
                .await
                .ok_or_else(|| capnp::Error::disconnected("closed".into()))
        }
        .boxed_local()
    }
}
fn pair() -> (Rc<Channel>, Rc<Channel>) {
    let (a_tx, a_rx) = futures::channel::mpsc::unbounded();
    let (b_tx, b_rx) = futures::channel::mpsc::unbounded();
    (
        Rc::new(Channel {
            input: futures::lock::Mutex::new(a_rx),
            output: b_tx,
        }),
        Rc::new(Channel {
            input: futures::lock::Mutex::new(b_rx),
            output: a_tx,
        }),
    )
}
struct Calculator {
    notifications: Rc<Cell<i32>>,
}
impl dynamic::Server for Calculator {
    fn allow_cancellation(&self) -> bool {
        true
    }
    fn call(self: Rc<Self>, mut context: dynamic::CallContext) -> Promise<(), capnp::Error> {
        Promise::from_future(async move {
            match context.method()?.get_proto().get_name()?.to_str()? {
                "add" => {
                    let p = context.get_params()?;
                    let (Value::Int32(x), Value::Int32(y)) = (p.get_named("x")?, p.get_named("y")?)
                    else {
                        panic!()
                    };
                    context
                        .get_results()?
                        .set_named("value", Value::Int32(x + y))?;
                }
                "notify" => {
                    let Value::Int32(v) = context.get_params()?.get_named("value")? else {
                        panic!()
                    };
                    self.notifications.set(v);
                }
                "fail" => return Err(capnp::Error::failed("expected test failure".into())),
                _ => panic!(),
            }
            Ok(())
        })
    }
}
fn service_schema() -> ServiceSchema {
    let parsed = common::schemas();
    let id = common::schema(&parsed, "Calculator").id();
    ServiceSchema::new(common::loader(), id).unwrap()
}
#[test]
fn bidirectional_calls_notifications_errors_and_driver_shutdown() {
    let mut pool = futures::executor::LocalPool::new();
    let schema = service_schema();
    let notified = Rc::new(Cell::new(0));
    let (a, b) = pair();
    let local = capnp_rpc::new_loaded_client(
        Calculator {
            notifications: notified.clone(),
        },
        schema.clone(),
    );
    let (left, left_driver) = JsonRpc::new(
        a,
        Some(Endpoint {
            schema: schema.clone(),
            client: Client::new(local.as_client_hook().add_ref()),
        }),
    )
    .unwrap();
    let (right, right_driver) = JsonRpc::new(
        b,
        Some(Endpoint {
            schema: schema.clone(),
            client: local,
        }),
    )
    .unwrap();
    let peer = left.get_peer(schema.clone()).unwrap();
    let reverse = right.get_peer(schema.clone()).unwrap();
    pool.spawner()
        .spawn_local(async move {
            let _ = left_driver.await;
        })
        .unwrap();
    pool.spawner()
        .spawn_local(async move {
            let _ = right_driver.await;
        })
        .unwrap();
    pool.run_until(async {
        let client = schema.reflect(peer).unwrap();
        let reverse = schema.reflect(reverse).unwrap();
        let mut a = client.new_request("add").unwrap();
        a.get().unwrap().set_named("x", Value::Int32(20)).unwrap();
        a.get().unwrap().set_named("y", Value::Int32(22)).unwrap();
        let mut b = reverse.new_request("add").unwrap();
        b.get().unwrap().set_named("x", Value::Int32(-10)).unwrap();
        let (a, b) = futures::join!(a.send().unwrap().resolve(), b.send().unwrap().resolve());
        assert!(matches!(
            a.unwrap().get().unwrap().get_named("value").unwrap(),
            Value::Int32(42)
        ));
        assert!(matches!(
            b.unwrap().get().unwrap().get_named("value").unwrap(),
            Value::Int32(-10)
        ));
        let mut n = client.new_request("notify").unwrap();
        n.get()
            .unwrap()
            .set_named("value", Value::Int32(17))
            .unwrap();
        n.send().unwrap().resolve().await.unwrap();
        let e = client
            .new_request("fail")
            .unwrap()
            .send()
            .unwrap()
            .resolve()
            .await
            .err()
            .unwrap();
        assert!(e.to_string().contains("expected test failure"));
    });
    pool.run_until_stalled();
    assert_eq!(notified.get(), 17);
}
#[test]
fn protocol_errors_unknown_method_invalid_params_and_canceled_calls() {
    let mut pool = futures::executor::LocalPool::new();
    let schema = service_schema();
    let (a, b) = pair();
    let client = capnp_rpc::new_loaded_client(
        Calculator {
            notifications: Rc::new(Cell::new(0)),
        },
        schema.clone(),
    );
    let (_, driver) = JsonRpc::new(a, Some(Endpoint { schema, client })).unwrap();
    pool.spawner()
        .spawn_local(async move {
            let _ = driver.await;
        })
        .unwrap();
    pool.run_until(async {
        for (input, code) in [
            ("{", -32700),
            (r#"{"jsonrpc":"1.0","id":1}"#, -32600),
            (
                r#"{"jsonrpc":"2.0","method":"missing","id":"abc","params":{}}"#,
                -32601,
            ),
            (
                r#"{"jsonrpc":"2.0","method":"sum","id":3,"params":{"x":"not-an-int"}}"#,
                -32602,
            ),
        ] {
            b.send(input).await.unwrap();
            let output = b.receive().await.unwrap();
            let v: serde_json::Value = serde_json::from_str(&output).unwrap();
            assert_eq!(v["error"]["code"], code);
        }
    });
}
#[test]
fn dropping_driver_rejects_pending_calls() {
    let schema = service_schema();
    let (a, _) = pair();
    let (rpc, driver) = JsonRpc::new(a, None).unwrap();
    let peer = rpc.get_peer(schema.clone()).unwrap();
    drop(driver);
    futures::executor::block_on(async {
        let c = schema.reflect(peer).unwrap();
        assert!(c
            .new_request("add")
            .unwrap()
            .send()
            .unwrap()
            .resolve()
            .await
            .is_err());
    });
    assert!(rpc.error().is_some());
}
#[derive(Clone, Default)]
struct Writer(Rc<RefCell<Vec<u8>>>);
impl futures::AsyncWrite for Writer {
    fn poll_write(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
        b: &[u8],
    ) -> std::task::Poll<std::io::Result<usize>> {
        self.0.borrow_mut().extend_from_slice(b);
        std::task::Poll::Ready(Ok(b.len()))
    }
    fn poll_flush(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Ready(Ok(()))
    }
    fn poll_close(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        std::task::Poll::Ready(Ok(()))
    }
}
#[test]
fn content_length_framing_utf8_multiple_messages_and_bounds() {
    futures::executor::block_on(async {
        let writer = Writer::default();
        let bytes=b"Content-Length: 2\r\nContent-Type: application/json\r\n\r\n{}content-length: 4\r\n\r\nnull".to_vec();
        let t = ContentLengthTransport::new(futures::io::Cursor::new(bytes), writer.clone());
        assert_eq!(t.receive().await.unwrap(), "{}");
        assert_eq!(t.receive().await.unwrap(), "null");
        t.send("\"é\"").await.unwrap();
        assert_eq!(
            &*writer.0.borrow(),
            "Content-Length: 4\r\n\r\n\"é\"".as_bytes()
        );
        for bytes in [
            "Content-Length: 999999999\r\n\r\n",
            "Content-Length: 1\r\nContent-Length: 1\r\n\r\nx",
            "X: y\r\n\r\n",
            "Content-Length: 4\r\n\r\na",
            "Content-Length: -1\r\n\r\n",
        ] {
            let t = ContentLengthTransport::new(
                futures::io::Cursor::new(bytes.as_bytes().to_vec()),
                Writer::default(),
            );
            assert!(t.receive().await.is_err(), "{bytes}");
        }
        let mut t = ContentLengthTransport::new(
            futures::io::Cursor::new(b"Long-Header: hello\r\n\r\n".to_vec()),
            Writer::default(),
        );
        t.max_header_bytes = 8;
        assert!(t.receive().await.is_err());
    })
}

#[test]
fn canceling_a_partial_transport_write_poison_closes_the_driver() {
    struct Blocked {
        started: Cell<bool>,
    }
    impl Transport for Blocked {
        fn send<'a>(&'a self, _: &'a str) -> LocalBoxFuture<'a, capnp::Result<()>> {
            async {
                self.started.set(true);
                futures::future::pending().await
            }
            .boxed_local()
        }
        fn receive(&self) -> LocalBoxFuture<'_, capnp::Result<String>> {
            futures::future::pending().boxed_local()
        }
    }
    let schema = service_schema();
    let transport = Rc::new(Blocked {
        started: Cell::new(false),
    });
    let (rpc, driver) = JsonRpc::new(transport.clone(), None).unwrap();
    let peer = schema
        .reflect(rpc.get_peer(schema.clone()).unwrap())
        .unwrap();
    let mut call = peer
        .new_request("add")
        .unwrap()
        .send()
        .unwrap()
        .resolve()
        .boxed_local();
    futures::executor::block_on(futures::future::poll_fn(|cx| {
        assert!(call.as_mut().poll(cx).is_pending());
        if transport.started.get() {
            std::task::Poll::Ready(())
        } else {
            cx.waker().wake_by_ref();
            std::task::Poll::Pending
        }
    }));
    drop(call);
    assert!(futures::executor::block_on(driver).is_err());
    assert!(rpc.error().is_some());
}
