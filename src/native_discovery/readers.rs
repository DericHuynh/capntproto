//! Ordered, explicitly delegated discovery readers with bounded control-route
//! failover. A successful response (including invalid data) is authoritative.
use super::*;

const MAX_READERS: usize = 4;

#[derive(Clone, Copy, Debug)]
pub struct DiscoveryOptions {
    /// Maximum time spent waiting on one reader (default three seconds).
    pub attempt_timeout: Duration,
    /// Total lookup budget across all readers (default six seconds, at most 30).
    pub timeout: Duration,
}
impl Default for DiscoveryOptions {
    fn default() -> Self {
        Self {
            attempt_timeout: Duration::from_secs(3),
            timeout: Duration::from_secs(6),
        }
    }
}

/// An ordered set of one to four independently delegated directory readers.
/// Each lookup tries each reader at most once, only failing over on disconnect,
/// overload or timeout. Absence, rejection and invalid records terminate it.
/// These capabilities are trusted authorities, not automatically synchronized
/// replicas: revoke a binding at every authority that can publish it.
#[derive(Clone)]
pub struct Discovery {
    readers: Rc<[directory::Client]>,
    options: DiscoveryOptions,
}
impl From<directory::Client> for Discovery {
    fn from(reader: directory::Client) -> Self {
        Self {
            readers: vec![reader].into(),
            options: DiscoveryOptions::default(),
        }
    }
}
enum Query<'a> {
    Name(&'a str),
    Host([u8; 32]),
}
impl Discovery {
    pub fn new(readers: Vec<directory::Client>, options: DiscoveryOptions) -> capnp::Result<Self> {
        if readers.is_empty()
            || readers.len() > MAX_READERS
            || options.attempt_timeout.is_zero()
            || options.attempt_timeout > options.timeout
            || options.timeout > Duration::from_secs(30)
        {
            return Err(failed("invalid Native discovery reader set or deadline"));
        }
        Ok(Self {
            readers: readers.into(),
            options,
        })
    }

    pub async fn resolve(&self, recipient: [u8; 32], name: &str) -> capnp::Result<Resolved> {
        if !name_ok(name) {
            return Err(failed("invalid discovery name"));
        }
        self.read(recipient, Query::Name(name)).await
    }

    /// Resolve a pinned key. A returned binding cannot substitute another host.
    pub async fn lookup(&self, recipient: [u8; 32], host: [u8; 32]) -> capnp::Result<Resolved> {
        if host == [0; 32] || host == recipient {
            return Err(failed("invalid discovery host"));
        }
        self.read(recipient, Query::Host(host)).await
    }

    async fn read(&self, recipient: [u8; 32], query: Query<'_>) -> capnp::Result<Resolved> {
        if recipient == [0; 32] {
            return Err(failed("invalid discovery recipient"));
        }
        let deadline = Instant::now() + self.options.timeout;
        let mut error = timed_out();
        for reader in self.readers.iter() {
            let started = Instant::now();
            if started >= deadline {
                return Err(timed_out());
            }
            let until = deadline.min(started + self.options.attempt_timeout);
            // Separate transport/RPC errors from decoding errors. A malformed
            // response is never a reason to consult a different authority.
            let response = tokio::time::timeout_at(until, async {
                match query {
                    Query::Name(name) => {
                        let mut request = reader.resolve_request();
                        request.get().set_name(name);
                        let response = request.send().promise.await?;
                        Ok(response
                            .get()
                            .and_then(|r| decode(r.get_record()?, recipient, started)))
                    }
                    Query::Host(host) => {
                        let mut request = reader.lookup_request();
                        request.get().set_host(&host);
                        let response = request.send().promise.await?;
                        Ok(response.get().and_then(|r| {
                            let resolved = decode(r.get_record()?, recipient, started)?;
                            if resolved.binding().host != host {
                                return Err(failed("directory changed pinned Native host"));
                            }
                            Ok(resolved)
                        }))
                    }
                }
            })
            .await;
            // Tokio may poll a ready response before its timer. The inclusive
            // deadline also rejects that response after an executor stall.
            if Instant::now() >= deadline {
                return Err(timed_out());
            }
            let response = if Instant::now() >= until {
                Err(timed_out())
            } else {
                response.unwrap_or_else(|_| Err(timed_out()))
            };
            match response {
                Ok(binding) => return binding,
                Err(e) => {
                    if !matches!(
                        e.kind,
                        capnp::ErrorKind::Disconnected | capnp::ErrorKind::Overloaded
                    ) {
                        return Err(e);
                    }
                    error = e;
                }
            }
        }
        Err(error)
    }
}
fn timed_out() -> Error {
    Error::disconnected("Native discovery timed out".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::FutureExt;
    use std::{cell::Cell, future::Future, pin::Pin};
    use tokio::sync::oneshot;

    type Reply = capnp::Result<u8>;
    #[derive(Default)]
    struct Reader {
        calls: Cell<usize>,
        reply: RefCell<Option<oneshot::Sender<Reply>>>,
    }
    impl Reader {
        async fn read(&self, mut out: record::Builder<'_>) -> capnp::Result<()> {
            self.calls.set(self.calls.get() + 1);
            let (tx, rx) = oneshot::channel();
            assert!(self.reply.borrow_mut().replace(tx).is_none());
            let kind = rx.await.map_err(|_| failed("test reader gone"))??;
            out.set_host(&[if kind == 1 { 2 } else { 3 }; 32]);
            out.set_recipient(&[if kind == 2 { 4 } else { 1 }; 32]);
            out.set_address("127.0.0.1:9000");
            out.set_generation(1);
            out.set_remaining_millis(if kind == 3 { 1 } else { 60_000 });
            struct Provider;
            impl provisioner::Server for Provider {}
            out.set_provider(capnp_rpc::new_client(Provider));
            Ok(())
        }
        fn respond(&self, reply: Reply) {
            // A response after cancellation is allowed to have no receiver.
            if let Some(tx) = self.reply.borrow_mut().take() {
                let _ = tx.send(reply);
            }
        }
    }
    impl directory::Server for Reader {
        async fn lookup(
            self: Rc<Self>,
            params: directory::LookupParams,
            mut results: directory::LookupResults,
        ) -> capnp::Result<()> {
            assert_eq!(params.get()?.get_host()?, &[2; 32]);
            self.read(results.get().init_record()).await
        }
        async fn resolve(
            self: Rc<Self>,
            params: directory::ResolveParams,
            mut results: directory::ResolveResults,
        ) -> capnp::Result<()> {
            assert_eq!(params.get()?.get_name()?.to_str()?, "service");
            self.read(results.get().init_record()).await
        }
    }
    fn setup(count: usize, options: DiscoveryOptions) -> (Discovery, Vec<Rc<Reader>>) {
        let readers: Vec<_> = (0..count).map(|_| Rc::new(Reader::default())).collect();
        let clients = readers
            .iter()
            .map(|r| capnp_rpc::new_client_from_rc(r.clone()))
            .collect();
        (Discovery::new(clients, options).unwrap(), readers)
    }
    type Lookup<'a> = Option<Pin<Box<dyn Future<Output = capnp::Result<Resolved>> + 'a>>>;
    async fn poll(future: &mut Lookup<'_>, result: &mut Option<capnp::Result<Resolved>>) {
        for _ in 0..8 {
            if let Some(f) = future.as_mut() {
                if let Some(r) = f.as_mut().now_or_never() {
                    *result = Some(r);
                    *future = None;
                }
            }
            tokio::task::yield_now().await;
        }
    }
    fn options() -> DiscoveryOptions {
        DiscoveryOptions {
            attempt_timeout: Duration::from_secs(1),
            timeout: Duration::from_secs(3),
        }
    }
    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn replay_tlc_discovery_failover() {
        use reproto_test_support::verification::exploration;
        let config = include_str!("../../verification/NativeDiscoveryFailover.cfg");
        let live = config.replace("SPECIFICATION Spec", "SPECIFICATION LiveSpec")
            + "\nPROPERTY Terminates\n";
        exploration::controls(
            "verification/NativeDiscoveryFailover.tla",
            "native-discovery-failover",
            config,
            &[
                ("retryDenied", "AuthoritativeStop"),
                ("acceptInvalid", "Pinned"),
                ("lateCommit", "NoLateCommit"),
            ],
            Some(&live),
        )
        .unwrap();
        let traces = exploration::traces(
            "verification/NativeDiscoveryFailover.tla",
            "native-discovery-failover",
            config,
        )
        .unwrap();
        tokio::task::LocalSet::new()
            .run_until(async {
                for trace in traces {
                    let (discovery, readers) = setup(3, options());
                    let mut future: Lookup<'_> = Some(Box::pin(discovery.lookup([1; 32], [2; 32])));
                    let mut result = None;
                    let started = Instant::now();
                    poll(&mut future, &mut result).await;
                    let mut reader = 0;
                    for state in trace {
                        match state["event"] {
                            1 => readers[reader]
                                .respond(Err(Error::disconnected("route lost".into()))),
                            2 => readers[reader]
                                .respond(Err(Error::overloaded("reader busy".into()))),
                            3 => tokio::time::advance(Duration::from_secs(1)).await,
                            4 => readers[reader].respond(Ok(1)),
                            5 => readers[reader].respond(Err(failed("absent"))),
                            6 => readers[reader].respond(Ok(0)),
                            7 => {
                                tokio::time::advance(
                                    (started + Duration::from_secs(3))
                                        .saturating_duration_since(Instant::now()),
                                )
                                .await
                            }
                            8 => {
                                future = None;
                            }
                            9 => {
                                for r in &readers {
                                    r.respond(Ok(1));
                                }
                            }
                            e => panic!("unexpected event {e}"),
                        }
                        poll(&mut future, &mut result).await;
                        reader = state["reader"] as usize - 1;
                        assert_eq!(
                            readers.iter().map(|r| r.calls.get()).sum::<usize>(),
                            reader + 1,
                            "{state:?}"
                        );
                        assert!(readers.iter().all(|r| r.calls.get() <= 1));
                        let phase = match &result {
                            Some(Ok(binding)) => {
                                assert_eq!(binding.binding().host, [2; 32]);
                                1
                            }
                            Some(Err(_)) => 2,
                            None if future.is_none() => 3,
                            None => 0,
                        };
                        assert_eq!(phase, state["phase"], "{state:?}");
                    }
                }
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn late_ready_responses_foreign_records_and_transit_expiry_fail_closed() {
        tokio::task::LocalSet::new()
            .run_until(async {
                // A ready reply polled exactly at an attempt deadline must be
                // discarded before consulting the next authority.
                let (discovery, readers) = setup(2, options());
                let mut future: Lookup<'_> = Some(Box::pin(discovery.lookup([1; 32], [2; 32])));
                let mut result = None;
                poll(&mut future, &mut result).await;
                readers[0].respond(Ok(1));
                tokio::time::advance(Duration::from_secs(1)).await;
                poll(&mut future, &mut result).await;
                assert!(result.is_none());
                assert_eq!(readers[1].calls.get(), 1);
                readers[1].respond(Ok(1));
                poll(&mut future, &mut result).await;
                assert!(result.unwrap().is_ok());
                // Neither a foreign recipient nor a response whose advertised
                // lifetime elapsed in transit can trigger fallback.
                for kind in [2, 3] {
                    let (discovery, readers) = setup(2, options());
                    let mut future: Lookup<'_> =
                        Some(Box::pin(discovery.resolve([1; 32], "service")));
                    let mut result = None;
                    poll(&mut future, &mut result).await;
                    tokio::time::advance(Duration::from_millis(5)).await;
                    readers[0].respond(Ok(kind));
                    poll(&mut future, &mut result).await;
                    assert!(result.unwrap().is_err());
                    assert_eq!(readers[1].calls.get(), 0);
                }
                // Overall deadline takes precedence over a ready valid response.
                let (discovery, readers) = setup(2, options());
                let mut future: Lookup<'_> = Some(Box::pin(discovery.resolve([1; 32], "service")));
                let mut result = None;
                poll(&mut future, &mut result).await;
                readers[0].respond(Ok(1));
                tokio::time::advance(Duration::from_secs(3)).await;
                poll(&mut future, &mut result).await;
                assert!(result.unwrap().is_err());
                assert_eq!(readers[1].calls.get(), 0);
            })
            .await;
    }
    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn validates_reader_limits_and_queries_before_invocation() {
        assert!(Discovery::new(vec![], options()).is_err());
        for count in [1, 4, 5] {
            let readers = (0..count)
                .map(|_| capnp_rpc::new_client(Reader::default()))
                .collect();
            assert_eq!(Discovery::new(readers, options()).is_ok(), count <= 4);
        }
        for (attempt, total) in [(0, 3), (4, 3), (1, 31), (1, 0)] {
            assert!(Discovery::new(
                vec![capnp_rpc::new_client(Reader::default())],
                DiscoveryOptions {
                    attempt_timeout: Duration::from_secs(attempt),
                    timeout: Duration::from_secs(total)
                }
            )
            .is_err());
        }
        let (d, r) = setup(1, options());
        assert!(d.resolve([1; 32], "bad name").await.is_err());
        assert!(d.resolve([0; 32], "service").await.is_err());
        assert!(d.lookup([1; 32], [0; 32]).await.is_err());
        assert!(d.lookup([1; 32], [1; 32]).await.is_err());
        assert_eq!(r[0].calls.get(), 0);
    }

    #[tokio::test(flavor = "current_thread", start_paused = true)]
    async fn four_readers_are_bounded_and_subsequent_lookups_start_fresh() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let (discovery, readers) = setup(4, options());
                // Cancellation before the first poll sends no lookup.
                drop(discovery.resolve([1; 32], "service"));
                assert!(readers.iter().all(|r| r.calls.get() == 0));
                let mut future: Lookup<'_> = Some(Box::pin(discovery.lookup([1; 32], [2; 32])));
                let mut result = None;
                poll(&mut future, &mut result).await;
                for r in &readers[..3] {
                    r.respond(Err(Error::overloaded("busy".into())));
                    poll(&mut future, &mut result).await;
                    assert!(result.is_none());
                }
                readers[3].respond(Ok(1));
                poll(&mut future, &mut result).await;
                assert!(result.unwrap().is_ok());
                assert!(readers.iter().all(|r| r.calls.get() == 1));
                // A healthy preferred reader is tried again. No cached success,
                // failure, name binding or mutable failover cursor leaks across calls.
                let mut future: Lookup<'_> = Some(Box::pin(discovery.resolve([1; 32], "service")));
                let mut result = None;
                poll(&mut future, &mut result).await;
                readers[0].respond(Ok(1));
                poll(&mut future, &mut result).await;
                assert!(result.unwrap().is_ok());
                assert_eq!(readers[0].calls.get(), 2);
                assert!(readers[1..].iter().all(|r| r.calls.get() == 1));
                // Unsupported methods are authoritative, not availability failures.
                let mut future: Lookup<'_> = Some(Box::pin(discovery.resolve([1; 32], "service")));
                let mut result = None;
                poll(&mut future, &mut result).await;
                readers[0].respond(Err(Error::unimplemented("unsupported".into())));
                poll(&mut future, &mut result).await;
                assert_eq!(
                    result.unwrap().err().unwrap().kind,
                    capnp::ErrorKind::Unimplemented
                );
                assert!(readers[1..].iter().all(|r| r.calls.get() == 1));
            })
            .await;
    }
}
