mod common;
use capnp::Result;
use capntproto_compat::{
    byte_stream::{ByteStreamFactory, ExplicitEndOutputStream, OutputStream},
    byte_stream_capnp::byte_stream as wire,
    http::{self, HttpService, ResponseSender},
    websocket::{self, Message, WebSocket},
};
use futures::{future::LocalBoxFuture, AsyncReadExt, AsyncWriteExt, FutureExt};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
#[derive(Default)]
struct Output {
    bytes: RefCell<Vec<u8>>,
    ends: Cell<u32>,
    aborts: Cell<u32>,
    tls: RefCell<Vec<String>>,
}
impl OutputStream for Output {
    fn write<'a>(&'a self, b: &'a [u8]) -> LocalBoxFuture<'a, Result<()>> {
        async move {
            self.bytes.borrow_mut().extend_from_slice(b);
            Ok(())
        }
        .boxed_local()
    }
    fn end(&self) -> LocalBoxFuture<'_, Result<()>> {
        async move {
            self.ends.set(self.ends.get() + 1);
            Ok(())
        }
        .boxed_local()
    }
    fn abort(&self) {
        self.aborts.set(self.aborts.get() + 1);
    }
    fn start_tls<'a>(&'a self, h: &'a str) -> LocalBoxFuture<'a, Result<()>> {
        async move {
            self.tls.borrow_mut().push(h.into());
            Ok(())
        }
        .boxed_local()
    }
}
#[test]
fn byte_stream_wire_calls_local_shortening_explicit_end_and_abort() {
    futures::executor::block_on(async {
        let factory = ByteStreamFactory::new();
        let out = Rc::new(Output::default());
        let client = factory.from_output(out.clone());
        let mut r = client.write_request();
        r.get().set_bytes(b"rpc");
        r.send().await.unwrap();
        let mut stream = ExplicitEndOutputStream::new(factory.to_output(client.clone()));
        stream.write(b"local").await.unwrap();
        stream.start_tls("example.test").await.unwrap();
        stream.end().await.unwrap();
        drop(stream);
        drop(client);
        assert_eq!(&*out.bytes.borrow(), b"rpclocal");
        assert_eq!(out.ends.get(), 1);
        assert_eq!(out.aborts.get(), 0);
        assert_eq!(&*out.tls.borrow(), &["example.test"]);
        let out = Rc::new(Output::default());
        drop(ExplicitEndOutputStream::new(
            factory.to_output(factory.from_output(out.clone())),
        ));
        assert_eq!(out.ends.get(), 0);
        assert_eq!(out.aborts.get(), 1);
    })
}
struct Callback {
    next: wire::Client,
    ended: Cell<Option<u64>>,
    reached: Cell<u32>,
}
impl wire::substream_callback::Server for Callback {
    async fn ended(
        self: Rc<Self>,
        p: wire::substream_callback::EndedParams,
        _: wire::substream_callback::EndedResults,
    ) -> Result<()> {
        self.ended.set(Some(p.get()?.get_byte_count()));
        Ok(())
    }
    async fn reached_limit(
        self: Rc<Self>,
        _: wire::substream_callback::ReachedLimitParams,
        mut r: wire::substream_callback::ReachedLimitResults,
    ) -> Result<()> {
        self.reached.set(self.reached.get() + 1);
        r.get().set_next(self.next.clone());
        Ok(())
    }
}
#[test]
fn substream_limit_splits_write_redirects_and_releases_parent() {
    futures::executor::block_on(async {
        let factory = ByteStreamFactory::new();
        let first = Rc::new(Output::default());
        let second = Rc::new(Output::default());
        let parent = factory.from_output(first.clone());
        let callback = Rc::new(Callback {
            next: factory.from_output(second.clone()),
            ended: Cell::new(None),
            reached: Cell::new(0),
        });
        let mut r = parent.get_substream_request();
        r.get().set_limit(3);
        r.get()
            .set_callback(capnp_rpc::new_client_from_rc(callback.clone()));
        let sub = r
            .send()
            .promise
            .await
            .unwrap()
            .get()
            .unwrap()
            .get_substream()
            .unwrap();
        assert!(factory
            .to_output(parent.clone())
            .write(b"bad")
            .await
            .is_err());
        let mut r = sub.write_request();
        r.get().set_bytes(b"abcdef");
        r.send().await.unwrap();
        let mut r = sub.write_request();
        r.get().set_bytes(b"g");
        r.send().await.unwrap();
        parent.end_request().send().promise.await.unwrap();
        sub.end_request().send().promise.await.unwrap();
        assert_eq!(&*first.bytes.borrow(), b"abc");
        assert_eq!(&*second.bytes.borrow(), b"defg");
        assert_eq!(callback.reached.get(), 1);
        assert_eq!(first.ends.get(), 1);
        assert_eq!(second.ends.get(), 1);
    })
}
#[test]
fn substream_early_end_counts_bytes_without_ending_parent() {
    futures::executor::block_on(async {
        let factory = ByteStreamFactory::new();
        let output = Rc::new(Output::default());
        let parent = factory.from_output(output.clone());
        let callback = Rc::new(Callback {
            next: parent.clone(),
            ended: Cell::new(None),
            reached: Cell::new(0),
        });
        let mut r = parent.get_substream_request();
        r.get().set_limit(99);
        r.get()
            .set_callback(capnp_rpc::new_client_from_rc(callback.clone()));
        let sub = r
            .send()
            .promise
            .await
            .unwrap()
            .get()
            .unwrap()
            .get_substream()
            .unwrap();
        let mut r = sub.write_request();
        r.get().set_bytes(b"abc");
        r.send().await.unwrap();
        sub.end_request().send().promise.await.unwrap();
        assert_eq!(callback.ended.get(), Some(3));
        assert_eq!(output.ends.get(), 0);
        factory.to_output(parent).write(b"d").await.unwrap();
        assert_eq!(&*output.bytes.borrow(), b"abcd");
    })
}
#[derive(Default)]
struct Socket {
    incoming: RefCell<std::collections::VecDeque<Message>>,
    sent: RefCell<Vec<Message>>,
}
impl WebSocket for Socket {
    fn receive(&self, max: usize) -> LocalBoxFuture<'_, Result<Message>> {
        async move {
            let m = self
                .incoming
                .borrow_mut()
                .pop_front()
                .unwrap_or(Message::Close {
                    code: None,
                    reason: String::new(),
                });
            if matches!(&m,Message::Binary(b) if b.len()>max) {
                return Err(capnp::Error::failed("oversized".into()));
            }
            Ok(m)
        }
        .boxed_local()
    }
    fn send(&self, m: Message) -> LocalBoxFuture<'_, Result<()>> {
        async move {
            self.sent.borrow_mut().push(m);
            Ok(())
        }
        .boxed_local()
    }
}
#[test]
fn websocket_io_keeps_message_boundaries_partial_writes_close_and_validation() {
    futures::executor::block_on(async {
        let socket = Rc::new(Socket::default());
        let stream =
            websocket::WebSocketMessageStream::new(socket.clone(), Default::default(), 1024);
        let mut a = capnp::message::Builder::new_default();
        a.set_root::<capnp::text::Owned>("hello").unwrap();
        let bytes = capnp::serialize::write_message_to_words(&a);
        let mut io = stream.clone().into_io();
        for b in bytes.chunks(3) {
            io.write_all(b).await.unwrap();
        }
        io.write_all(&bytes).await.unwrap();
        io.close().await.unwrap();
        assert_eq!(
            socket.sent.borrow().as_slice(),
            &[
                Message::Binary(bytes.clone()),
                Message::Binary(bytes.clone()),
                Message::Close {
                    code: None,
                    reason: "Capnp connection closed".into()
                }
            ]
        );
        socket
            .incoming
            .borrow_mut()
            .push_back(Message::Binary(bytes.clone()));
        let reader = stream.try_read_message().await.unwrap().unwrap();
        assert_eq!(reader.get_root::<capnp::text::Reader>().unwrap(), "hello");
        let mut trailing = bytes.clone();
        trailing.extend(&bytes);
        for message in [
            Message::Text("hello".into()),
            Message::Binary(vec![0; 4]),
            Message::Binary(trailing),
        ] {
            socket.incoming.borrow_mut().push_back(message);
            assert!(stream.try_read_message().await.is_err());
        }
        let mut io = stream.into_io();
        io.write_all(&bytes[..5]).await.unwrap();
        assert!(io.close().await.is_err());
    })
}
struct Echo;
impl HttpService for Echo {
    fn request(
        &self,
        request: http::Request,
        mut body: http::Body,
        response: Rc<dyn ResponseSender>,
    ) -> LocalBoxFuture<'_, Result<()>> {
        async move {
            assert_eq!(request.url, "/echo");
            let mut bytes = vec![];
            body.read_to_end(&mut bytes).await?;
            let output = response
                .start_response(http::Response {
                    status_code: 200,
                    status_text: "OK".into(),
                    headers: vec![("X-Echo".into(), "yes".into())],
                    body_size: http::BodySize::Fixed(bytes.len() as u64),
                })
                .await?;
            output.write(&bytes).await?;
            output.end().await
        }
        .boxed_local()
    }
}
struct ResponseCapture {
    output: Rc<Output>,
    metadata: RefCell<Option<http::Response>>,
}
impl ResponseSender for ResponseCapture {
    fn start_response(
        &self,
        r: http::Response,
    ) -> LocalBoxFuture<'_, Result<Rc<dyn OutputStream>>> {
        async move {
            *self.metadata.borrow_mut() = Some(r);
            Ok(self.output.clone() as Rc<dyn OutputStream>)
        }
        .boxed_local()
    }
    fn start_websocket(
        &self,
        _: http::Headers,
        _: Rc<dyn http::WebSocketSink>,
    ) -> LocalBoxFuture<'_, Result<Rc<dyn http::WebSocketSink>>> {
        async { Err(capnp::Error::unimplemented("not in this test".into())) }.boxed_local()
    }
}
#[test]
fn http_request_body_pipelines_response_and_common_headers_roundtrip() {
    futures::executor::block_on(async {
        let factory = http::HttpOverCapnpFactory::default();
        let service = factory.to_service(factory.from_service(Rc::new(Echo)));
        let response = Rc::new(ResponseCapture {
            output: Rc::new(Output::default()),
            metadata: RefCell::new(None),
        });
        let bytes = vec![42; 200_000];
        service
            .request(
                http::Request {
                    method: http::HttpMethod::Post,
                    url: "/echo".into(),
                    headers: vec![("content-type".into(), "application/octet-stream".into())],
                    body_size: http::BodySize::Fixed(bytes.len() as u64),
                },
                Box::new(futures::io::Cursor::new(bytes.clone())),
                response.clone(),
            )
            .await
            .unwrap();
        assert_eq!(*response.output.bytes.borrow(), bytes);
        assert_eq!(response.output.ends.get(), 1);
        assert_eq!(response.output.aborts.get(), 0);
        assert_eq!(
            response.metadata.borrow().as_ref().unwrap().status_code,
            200
        );
        let empty = Rc::new(ResponseCapture {
            output: Rc::new(Output::default()),
            metadata: RefCell::new(None),
        });
        service
            .request(
                http::Request {
                    method: http::HttpMethod::Head,
                    url: "/echo".into(),
                    headers: vec![],
                    body_size: http::BodySize::Fixed(0),
                },
                Box::new(futures::io::Cursor::new(vec![])),
                empty.clone(),
            )
            .await
            .unwrap();
        assert_eq!(empty.output.ends.get(), 1);
        assert_eq!(empty.output.aborts.get(), 0);
        let headers = vec![
            ("Accept-Encoding".into(), "gzip, deflate".into()),
            ("X-Custom".into(), "one".into()),
            ("Set-Cookie".into(), "a=1".into()),
            ("Set-Cookie".into(), "b=2".into()),
        ];
        let mut m = capnp::message::Builder::new_default();
        http::encode_headers(&headers, m.initn_root(headers.len() as u32)).unwrap();
        assert_eq!(
            http::decode_headers(m.get_root_as_reader().unwrap()).unwrap(),
            headers
        );
    })
}

impl http::ConnectResponseSender for ResponseCapture {
    fn start_connect(&self, _: http::Response) -> LocalBoxFuture<'_, Result<()>> {
        async { panic!("unexpected CONNECT acceptance") }.boxed_local()
    }
    fn start_error(&self, r: http::Response) -> LocalBoxFuture<'_, Result<Rc<dyn OutputStream>>> {
        self.start_response(r)
    }
}

#[test]
fn connect_rejection_delivers_empty_and_nonempty_error_bodies() {
    struct Reject(Vec<u8>);
    impl HttpService for Reject {
        fn request(
            &self,
            _: http::Request,
            _: http::Body,
            _: Rc<dyn ResponseSender>,
        ) -> LocalBoxFuture<'_, Result<()>> {
            async { panic!("unexpected request") }.boxed_local()
        }
        fn connect(
            &self,
            request: http::ConnectRequest,
            _: Rc<dyn OutputStream>,
            response: Rc<dyn http::ConnectResponseSender>,
        ) -> LocalBoxFuture<'_, Result<http::Connection>> {
            async move {
                assert_eq!(request.host, "example.test:443");
                assert!(request.use_tls);
                let body = response
                    .start_error(http::Response {
                        status_code: 403,
                        status_text: "Forbidden".into(),
                        headers: vec![],
                        body_size: http::BodySize::Fixed(self.0.len() as u64),
                    })
                    .await?;
                body.write(&self.0).await?;
                body.end().await?;
                Ok(http::Connection {
                    up: Rc::new(Output::default()),
                    completion: async { Ok(()) }.boxed_local(),
                })
            }
            .boxed_local()
        }
    }
    futures::executor::block_on(async {
        for bytes in [vec![], b"access denied".to_vec()] {
            let factory = http::HttpOverCapnpFactory::default();
            let service = factory.to_service(factory.from_service(Rc::new(Reject(bytes.clone()))));
            let response = Rc::new(ResponseCapture {
                output: Rc::new(Output::default()),
                metadata: RefCell::new(None),
            });
            let connection = service
                .connect(
                    http::ConnectRequest {
                        host: "example.test:443".into(),
                        headers: vec![],
                        use_tls: true,
                    },
                    Rc::new(Output::default()),
                    response.clone(),
                )
                .await
                .unwrap();
            connection.completion.await.unwrap();
            assert_eq!(
                response.metadata.borrow().as_ref().unwrap().status_code,
                403
            );
            assert_eq!(*response.output.bytes.borrow(), bytes);
            assert_eq!(response.output.ends.get(), 1);
            assert_eq!(response.output.aborts.get(), 0);
        }
    });
}
#[test]
fn body_pipe_rejects_truncation_overflow_and_missing_explicit_end() {
    futures::executor::block_on(async {
        let (mut reader, output) = http::body_pipe(http::BodySize::Fixed(3));
        assert!(output.write(b"long").await.is_err());
        let write = async {
            output.write(b"ab").await.unwrap();
            assert!(output.end().await.is_err());
            output.abort();
        };
        let read = async {
            let mut bytes = vec![];
            assert!(reader.read_to_end(&mut bytes).await.is_err());
            assert_eq!(bytes, b"ab");
        };
        futures::join!(write, read);
        let (mut reader, output) = http::body_pipe(http::BodySize::Unknown);
        drop(output);
        let mut bytes = vec![];
        assert!(reader.read_to_end(&mut bytes).await.is_err());
    })
}

#[test]
fn zero_substream_limit_resolves_without_write_and_cancel_aborts_output() {
    futures::executor::block_on(async {
        let factory = ByteStreamFactory::new();
        let first = Rc::new(Output::default());
        let second = Rc::new(Output::default());
        let parent = factory.from_output(first.clone());
        let next = factory.from_output(second.clone());
        let callback = Rc::new(Callback {
            next: next.clone(),
            ended: Cell::new(None),
            reached: Cell::new(0),
        });
        let mut request = parent.get_substream_request();
        request.get().set_limit(0);
        request
            .get()
            .set_callback(capnp_rpc::new_client_from_rc(callback.clone()));
        let sub = request
            .send()
            .promise
            .await
            .unwrap()
            .get()
            .unwrap()
            .get_substream()
            .unwrap();
        sub.client.when_resolved().await.unwrap();
        drop(sub);
        assert_eq!(callback.reached.get(), 1);
        assert_eq!(second.aborts.get(), 0);
        factory.to_output(next).write(b"still open").await.unwrap();
        factory.to_output(parent).end().await.unwrap();
        assert_eq!(first.ends.get(), 1);
        struct Blocked(Cell<bool>);
        impl OutputStream for Blocked {
            fn write<'a>(&'a self, _: &'a [u8]) -> LocalBoxFuture<'a, Result<()>> {
                futures::future::pending().boxed_local()
            }
            fn end(&self) -> LocalBoxFuture<'_, Result<()>> {
                async { Ok(()) }.boxed_local()
            }
            fn abort(&self) {
                self.0.set(true);
            }
        }
        let blocked = Rc::new(Blocked(Cell::new(false)));
        let output = factory.to_output(factory.from_output(blocked.clone()));
        assert!(output.write(b"partial").now_or_never().is_none());
        assert!(blocked.0.get());
        assert!(output.write(b"later").await.is_err());
    });
}

#[test]
fn futures_io_close_and_explicit_legacy_drop_executor() {
    use capntproto_compat::byte_stream::OutputIo;
    futures::executor::block_on(async {
        let output = Rc::new(Output::default());
        let mut io = OutputIo::new(output.clone());
        io.write_all(&vec![7; 130_000]).await.unwrap();
        io.close().await.unwrap();
        drop(io);
        assert_eq!(output.bytes.borrow().len(), 130_000);
        assert_eq!(output.ends.get(), 1);
        assert_eq!(output.aborts.get(), 0);
    });
    struct Executor(RefCell<Vec<capnp::capability::Promise<(), capnp::Error>>>);
    impl capnp::capability::CallExecutor for Executor {
        fn spawn(&self, p: capnp::capability::Promise<(), capnp::Error>) -> Result<()> {
            self.0.borrow_mut().push(p);
            Ok(())
        }
    }
    let executor = Rc::new(Executor(RefCell::new(vec![])));
    let output = Rc::new(Output::default());
    drop(OutputIo::new(output.clone()).with_eof_on_drop(executor.clone()));
    assert_eq!(output.ends.get(), 0);
    let tasks = std::mem::take(&mut *executor.0.borrow_mut());
    futures::executor::block_on(async {
        for p in tasks {
            p.await.unwrap();
        }
    });
    assert_eq!(output.ends.get(), 1);
    assert_eq!(output.aborts.get(), 0);
}

#[test]
fn wrapping_an_unwrapped_capability_shortens_to_original_identity() {
    use capnp::capability::FromClientHook;
    futures::executor::block_on(async {
        let first = ByteStreamFactory::new();
        let other = ByteStreamFactory::new();
        let output = Rc::new(Output::default());
        let original = first.from_output(output.clone());
        for factory in [&first, &other] {
            let relay = factory.from_output(factory.to_output(original.clone()));
            relay.client.when_resolved().await.unwrap();
            let mut hook = relay.as_client_hook().add_ref();
            while let Some(next) = hook.get_resolved() {
                hook = next;
            }
            assert_eq!(hook.get_ptr(), original.as_client_hook().get_ptr());
            drop(relay);
            assert_eq!(output.aborts.get(), 0);
        }
        first.to_output(original).end().await.unwrap();
        assert_eq!(output.ends.get(), 1);
    });
}
