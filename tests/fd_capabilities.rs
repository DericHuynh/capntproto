#![cfg(unix)]
#[allow(dead_code)]
mod support;
use capnp::{
    capability::{Client, FromClientHook},
    traits::{HasTypeId, ImbueMut},
};
use capnp_rpc::{
    membrane::{Direction, Membrane, Policy},
    rpc_capnp::{cap_descriptor, message, return_},
    Connection, RpcSystem,
};
use capntproto_test_support::runtime_test_capnp::harness;
use std::{
    cell::RefCell,
    io::Write,
    os::{fd::OwnedFd, unix::fs::FileExt},
    rc::Rc,
};
use support::{Endpoint, Hub};

fn fd(value: u8) -> Rc<OwnedFd> {
    let mut file = tempfile::tempfile().unwrap();
    file.write_all(&[value]).unwrap();
    Rc::new(file.into())
}
fn value(fd: Option<Rc<OwnedFd>>) -> u32 {
    fd.map(|fd| {
        let file = std::fs::File::from(fd.try_clone().unwrap());
        let mut byte = [0];
        file.read_exact_at(&mut byte, 0).unwrap();
        u32::from(byte[0])
    })
    .unwrap_or(0)
}
struct Permit(bool);
impl Policy for Permit {
    fn call(&self, _: Direction, _: u64, _: u16, _: &Client) -> capnp::Result<Option<Client>> {
        Ok(None)
    }
    fn allow_fd_passthrough(&self) -> bool {
        self.0
    }
}
#[derive(Clone)]
struct Sink(Rc<RefCell<Vec<harness::Client>>>);
impl harness::Server for Sink {
    async fn fd_caps(
        self: Rc<Self>,
        p: harness::FdCapsParams,
        mut r: harness::FdCapsResults,
    ) -> capnp::Result<()> {
        let list = p.get()?.get_caps()?;
        let mut result = r.get().init_caps(list.len());
        for i in 0..list.len() {
            let cap = list.get(i)?;
            self.0.borrow_mut().push(cap.clone());
            result.set(i, cap.into_client_hook());
        }
        Ok(())
    }
}
struct Dummy;
impl harness::Server for Dummy {}
enum Peer {
    Memory(Endpoint),
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    Unix {
        connection: Box<dyn Connection<capnp_rpc::rpc_twoparty_capnp::Side>>,
        task: tokio::task::JoinHandle<capnp::Result<()>>,
    },
}
impl Drop for Peer {
    fn drop(&mut self) {
        #[cfg(any(target_os = "linux", target_os = "macos"))]
        if let Self::Unix { task, .. } = self {
            task.abort();
        }
    }
}
impl Peer {
    fn new_outgoing_message(&mut self, words: u32) -> Box<dyn capnp_rpc::OutgoingMessage> {
        match self {
            Self::Memory(c) => c.new_outgoing_message(words),
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            Self::Unix { connection, .. } => connection.new_outgoing_message(words),
        }
    }
    fn send(&mut self, fill: impl FnOnce(message::Builder<'_>)) {
        let mut message = self.new_outgoing_message(64);
        fill(message.get_body().unwrap().init_as());
        drop(message.send());
    }
    async fn recv(&mut self) -> Box<dyn capnp_rpc::IncomingMessage> {
        match self {
            Self::Memory(c) => c.recv().await,
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            Self::Unix { connection, .. } => connection
                .receive_incoming_message()
                .await
                .unwrap()
                .unwrap(),
        }
    }
}
struct Fixture {
    _hub: Option<Rc<RefCell<Hub>>>,
    peer: Peer,
    task: tokio::task::JoinHandle<capnp::Result<()>>,
    caps: Rc<RefCell<Vec<harness::Client>>>,
    service: u32,
    next: u32,
    membrane: Membrane,
    wrapped: Vec<harness::Client>,
    observed: Vec<Option<Rc<OwnedFd>>>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Fixture {
    async fn new(allow: bool) -> Self {
        Self::new_on(allow, false).await
    }
    async fn new_on(allow: bool, unix: bool) -> Self {
        let caps = Rc::new(RefCell::new(Vec::new()));
        let service: harness::Client = capnp_rpc::new_client(Sink(caps.clone()));
        let (hub, mut peer, task) = if unix {
            #[cfg(not(any(target_os = "linux", target_os = "macos")))]
            panic!("Linux or macOS Unix transport required");
            #[cfg(any(target_os = "linux", target_os = "macos"))]
            {
                use capnp_rpc::{rpc_twoparty_capnp::Side, VatNetwork};
                let (left, right) = tokio::net::UnixStream::pair().unwrap();
                let network =
                    capntproto::unix_rpc::VatNetwork::new(left, Side::Server, Default::default());
                let system = RpcSystem::new(Box::new(network), Some(service.client));
                let mut raw =
                    capntproto::unix_rpc::VatNetwork::new(right, Side::Client, Default::default());
                let connection = raw.connect(Side::Server).unwrap();
                let driver =
                    tokio::task::spawn_local(async move { raw.drive_until_shutdown().await });
                (
                    None,
                    Peer::Unix {
                        connection,
                        task: driver,
                    },
                    tokio::task::spawn_local(system),
                )
            }
        } else {
            let hub = Rc::new(RefCell::new(Hub::default()));
            let system = RpcSystem::new(Box::new(Hub::network(&hub, 1)), Some(service.client));
            let peer = Peer::Memory(hub.borrow_mut().connect(2, 1));
            (Some(hub), peer, tokio::task::spawn_local(system))
        };
        peer.send(|m| m.init_bootstrap().set_question_id(0));
        let m = peer.recv().await;
        let message::Return(r) = m
            .get_body()
            .unwrap()
            .get_as::<message::Reader>()
            .unwrap()
            .which()
            .unwrap()
        else {
            panic!()
        };
        let return_::Results(p) = r.unwrap().which().unwrap() else {
            panic!()
        };
        let cap_descriptor::SenderHosted(service) =
            p.unwrap().get_cap_table().unwrap().get(0).which().unwrap()
        else {
            panic!()
        };
        Self {
            _hub: hub,
            peer,
            task,
            caps,
            service,
            next: 1,
            membrane: Membrane::new(Rc::new(Permit(allow))),
            wrapped: vec![],
            observed: vec![],
        }
    }
    // Kind 0/1 imports a hosted/promise cap; kind 2 returns our authoritative
    // local bootstrap capability; kind 3 references an invalid local export.
    async fn deliver(
        &mut self,
        entries: &[(u32, u8)],
        fds: Vec<Rc<OwnedFd>>,
        kind: u32,
    ) -> Vec<u32> {
        let mut m = self.peer.new_outgoing_message(64);
        let id = self.next;
        self.next += 1;
        {
            let mut call = m
                .get_body()
                .unwrap()
                .init_as::<message::Builder>()
                .init_call();
            call.set_question_id(id);
            call.reborrow().init_target().set_imported_cap(self.service);
            call.set_interface_id(harness::Client::TYPE_ID);
            call.set_method_id(6);
            let mut payload = call.init_params();
            let mut table = vec![];
            {
                let mut content = payload.reborrow().get_content();
                content.imbue_mut(&mut table);
                let mut list = content
                    .init_as::<harness::fd_caps_params::Builder>()
                    .init_caps(entries.len() as u32);
                for i in 0..entries.len() {
                    list.set(
                        i as u32,
                        capnp_rpc::new_client::<harness::Client, _>(Dummy).into_client_hook(),
                    );
                }
            }
            let mut descriptors = payload.init_cap_table(entries.len() as u32);
            for (i, &(export, index)) in entries.iter().enumerate() {
                let mut d = descriptors.reborrow().get(i as u32);
                match kind {
                    0 => d.set_sender_hosted(export),
                    1 => d.set_sender_promise(export),
                    2 => d.set_receiver_hosted(self.service),
                    3 => d.set_receiver_hosted(u32::MAX),
                    _ => panic!(),
                }
                d.set_attached_fd(index);
            }
        }
        m.set_fds(fds);
        drop(m.send());
        loop {
            let mut m = self.peer.recv().await;
            let mut fds: Vec<_> = m
                .take_fds()
                .into_iter()
                .map(|fd| Some(Rc::new(fd)))
                .collect();
            if let message::Return(r) = m
                .get_body()
                .unwrap()
                .get_as::<message::Reader>()
                .unwrap()
                .which()
                .unwrap()
            {
                let r = r.unwrap();
                assert_eq!(r.get_answer_id(), id);
                let return_::Results(p) = r.which().unwrap() else {
                    panic!("expected successful capability echo")
                };
                let table = p.unwrap().get_cap_table().unwrap();
                let result = (0..table.len())
                    .map(|i| {
                        let index = table.get(i).get_attached_fd() as usize;
                        value(fds.get_mut(index).and_then(Option::take))
                    })
                    .collect();
                self.peer.send(|m| m.init_finish().set_question_id(id));
                return result;
            }
        }
    }
    fn wrap(&mut self) {
        self.wrapped = self.caps.borrow()[..2]
            .iter()
            .map(|c| self.membrane.export(c.clone()))
            .collect();
    }
    fn check(&self, state: &[u32]) {
        // delivered, reimported, observed, revoked, left, right, seenLeft, seenRight
        let caps = self.caps.borrow();
        assert_eq!(value(caps[0].client.hook.get_fd()), state[4]);
        assert_eq!(value(caps[1].client.hook.get_fd()), state[5]);
        if state[2] == 1 {
            assert_eq!(value(self.observed[0].clone()), state[6]);
            assert_eq!(value(self.observed[1].clone()), state[7]);
        }
        if state[3] == 1 {
            for cap in &self.wrapped {
                assert!(cap.client.hook.get_fd().is_none());
            }
        }
    }
}
async fn replay(case: &serde_json::Value, promise: bool, unix: bool) {
    let allow = case["allow"].as_bool().unwrap();
    let shared = case["shared"].as_bool().unwrap();
    let same = case["same"].as_bool().unwrap();
    let limit = case["limit"].as_u64().unwrap() as usize;
    let mut fixture = Fixture::new_on(allow, unix).await;
    for step in case["steps"].as_array().unwrap() {
        let state: Vec<u32> = serde_json::from_value(step["state"].clone()).unwrap();
        match step["action"].as_str().unwrap() {
            "deliver" => {
                let result = fixture
                    .deliver(
                        &[
                            (77, 0),
                            (if same { 77 } else { 78 }, if shared { 0 } else { 1 }),
                        ],
                        vec![fd(1), fd(2)].into_iter().take(limit).collect(),
                        u32::from(promise),
                    )
                    .await;
                // Cap-table aliases may collapse to one outgoing descriptor.
                assert!(result.iter().all(|v| *v == state[4] || *v == state[5]));
                fixture.wrap();
            }
            "reimport" => {
                fixture
                    .deliver(&[(77, 0)], vec![fd(3)], u32::from(promise))
                    .await;
            }
            "observe" => {
                fixture.observed = fixture
                    .wrapped
                    .iter()
                    .map(|c| c.client.hook.get_fd())
                    .collect();
            }
            "revoke" => fixture
                .membrane
                .revoke(capnp::Error::failed("revoked".into())),
            _ => panic!(),
        }
        fixture.check(&state);
    }
}
#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_fd_traces() {
    let path = capntproto_test_support::verification::input("CAPNTPROTO_FD_TRACES")
        .expect("prepare verified trace corpus");
    let cases: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    tokio::task::LocalSet::new()
        .run_until(async {
            for case in cases {
                for promise in [false, true] {
                    replay(&case, promise, false).await;
                    #[cfg(any(target_os = "linux", target_os = "macos"))]
                    replay(&case, promise, true).await;
                }
            }
        })
        .await;
}
#[tokio::test(flavor = "current_thread")]
async fn fd_authority_and_promise_resolution() {
    struct CountingPolicy(std::cell::Cell<usize>);
    impl Policy for CountingPolicy {
        fn call(&self, _: Direction, _: u64, _: u16, _: &Client) -> capnp::Result<Option<Client>> {
            Ok(None)
        }
        fn allow_fd_passthrough(&self) -> bool {
            self.0.set(self.0.get() + 1);
            true
        }
    }
    let policy = Rc::new(CountingPolicy(std::cell::Cell::new(0)));
    let boundary = Membrane::new(policy.clone());
    let plain: harness::Client = capnp_rpc::new_client(Dummy);
    assert!(boundary.export(plain).client.hook.get_fd().is_none());
    assert_eq!(
        policy.0.get(),
        0,
        "C++ does not query FD policy without an FD"
    );
    let local: harness::Client = capnp_rpc::new_fd_client(Dummy, fd(9).try_clone().unwrap());
    let wrapped = boundary.export(local);
    assert_eq!(value(wrapped.client.hook.get_fd()), 9);
    assert_eq!(policy.0.get(), 1);
    boundary.revoke(capnp::Error::failed("revoked".into()));
    assert!(wrapped.client.hook.get_fd().is_none());
    assert_eq!(policy.0.get(), 1);

    tokio::task::LocalSet::new()
        .run_until(async {
            let local: harness::Client =
                capnp_rpc::new_fd_client(Dummy, fd(9).try_clone().unwrap());
            assert_eq!(value(local.client.get_fd().await.unwrap()), 9);
            let (tx, rx) = futures::channel::oneshot::channel();
            let promised: harness::Client =
                capnp_rpc::new_future_client(async move { Ok(rx.await.unwrap()) });
            let get = promised.client.get_fd();
            tx.send(local.clone()).unwrap_or_else(|_| panic!());
            assert_eq!(value(get.await.unwrap()), 9);
            let broken: harness::Client =
                capnp_rpc::new_future_client(async { Err(capnp::Error::failed("broken".into())) });
            assert!(broken.client.get_fd().await.is_err());
            let block = Membrane::new(Rc::new(Permit(false)));
            assert!(block
                .export(local.clone())
                .client
                .get_fd()
                .await
                .unwrap()
                .is_none());
            let allow = Membrane::new(Rc::new(Permit(true)));
            let cap = allow.export(local);
            let escaped = cap.client.get_fd().await.unwrap();
            allow.revoke(capnp::Error::failed("revoked".into()));
            assert!(cap.client.get_fd().await.is_err());
            assert_eq!(value(escaped), 9);
            let mut fixture = Fixture::new(true).await;
            for kind in [2, 3] {
                assert_eq!(
                    fixture.deliver(&[(77, 0)], vec![fd(8)], kind).await,
                    vec![0]
                );
                assert!(fixture
                    .caps
                    .borrow()
                    .last()
                    .unwrap()
                    .client
                    .hook
                    .get_fd()
                    .is_none());
            }
            // An unresolved senderPromise can expose its descriptor immediately.
            fixture.deliver(&[(77, 0)], vec![fd(7)], 1).await;
            let cap = fixture.caps.borrow().last().unwrap().clone();
            assert_eq!(value(cap.client.get_fd().await.unwrap()), 7);
        })
        .await;
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[tokio::test(flavor = "current_thread")]
async fn unix_rpc_bootstrap_resolve_call_return_and_fallback() {
    struct Files(Rc<RefCell<Vec<u32>>>);
    impl harness::Server for Files {
        async fn echo(
            self: Rc<Self>,
            _: harness::EchoParams,
            mut r: harness::EchoResults,
        ) -> capnp::Result<()> {
            r.get().set_value(42);
            Ok(())
        }
        async fn bounce(
            self: Rc<Self>,
            p: harness::BounceParams,
            mut r: harness::BounceResults,
        ) -> capnp::Result<()> {
            let fd = p.get()?.get_cap()?.client.get_fd().await?;
            self.0.borrow_mut().push(value(fd));
            r.get().set_cap(capnp_rpc::new_fd_client(Dummy, fd_cap(10)));
            Ok(())
        }
        async fn fd_caps(
            self: Rc<Self>,
            p: harness::FdCapsParams,
            mut r: harness::FdCapsResults,
        ) -> capnp::Result<()> {
            let caps = p.get()?.get_caps()?;
            let mut out = r.get().init_caps(caps.len());
            for i in 0..caps.len() {
                out.set(i, caps.get(i)?.into_client_hook());
            }
            Ok(())
        }
    }
    fn fd_cap(byte: u8) -> OwnedFd {
        fd(byte).try_clone().unwrap()
    }
    tokio::task::LocalSet::new()
        .run_until(async {
            tokio::time::timeout(std::time::Duration::from_secs(5), async {
                for limit in [0, 16] {
                    for promised in [false, true] {
                        use capnp_rpc::rpc_twoparty_capnp::Side;
                        let (a, b) = tokio::net::UnixStream::pair().unwrap();
                        let options = capntproto::unix_rpc::Options {
                            max_fds: limit,
                            ..Default::default()
                        };
                        let (tx, rx) = futures::channel::oneshot::channel();
                        let seen = Rc::new(RefCell::new(vec![]));
                        let server: harness::Client =
                            capnp_rpc::new_fd_client(Files(seen.clone()), fd_cap(9));
                        let bootstrap = if promised {
                            capnp_rpc::new_future_client::<harness::Client>(async move {
                                Ok(rx.await.unwrap())
                            })
                        } else {
                            server.clone()
                        };
                        let server_system = RpcSystem::new(
                            Box::new(capntproto::unix_rpc::VatNetwork::new(
                                a,
                                Side::Server,
                                options,
                            )),
                            Some(bootstrap.client),
                        );
                        let mut caller_system = RpcSystem::new(
                            Box::new(capntproto::unix_rpc::VatNetwork::new(
                                b,
                                Side::Client,
                                options,
                            )),
                            None,
                        );
                        let remote: harness::Client = caller_system.bootstrap(Side::Server);
                        let tasks = [
                            tokio::task::spawn_local(server_system),
                            tokio::task::spawn_local(caller_system),
                        ];
                        if promised {
                            // Drive Bootstrap first so this goes through Resolve.cap,
                            // not a bootstrap reply that was already settled locally.
                            for _ in 0..16 {
                                tokio::task::yield_now().await;
                            }
                            tx.send(server).unwrap_or_else(|_| panic!());
                        }
                        assert_eq!(
                            value(remote.client.get_fd().await.unwrap()),
                            if limit > 0 { 9 } else { 0 }
                        );
                        assert_eq!(
                            remote
                                .echo_request()
                                .send()
                                .promise
                                .await
                                .unwrap()
                                .get()
                                .unwrap()
                                .get_value(),
                            42
                        );
                        let local: harness::Client = capnp_rpc::new_fd_client(Dummy, fd_cap(7));
                        let mut request = remote.bounce_request();
                        request.get().set_cap(local.clone());
                        let response = request.send().promise.await.unwrap();
                        let returned = response.get().unwrap().get_cap().unwrap();
                        assert_eq!(
                            value(returned.client.get_fd().await.unwrap()),
                            if limit > 0 { 10 } else { 0 }
                        );
                        assert_eq!(*seen.borrow(), vec![if limit > 0 { 7 } else { 0 }]);
                        let mut request = remote.fd_caps_request();
                        request
                            .get()
                            .init_caps(1)
                            .set(0, local.client.hook.add_ref());
                        let response = request.send().promise.await.unwrap();
                        let reflected = capnp::capability::get_resolved_cap(
                            response.get().unwrap().get_caps().unwrap().get(0).unwrap(),
                        )
                        .await;
                        assert_eq!(reflected.client.hook.get_ptr(), local.client.hook.get_ptr());
                        assert_eq!(value(reflected.client.get_fd().await.unwrap()), 7);
                        for task in tasks {
                            task.abort();
                        }
                    }
                }
            })
            .await
            .unwrap();
        })
        .await;
}
