//! Connection setup stays outside the protocol's per-call path.
use super::*;

/// Executor-neutral setup for an already connected byte stream.
///
/// Defaults to legacy RPC and its reader limits. Portable storage remains an
/// explicit choice. Both roles use the same options; neither constructor spawns
/// tasks. Poll the returned driver or use its `run` / `run_until` methods.
///
/// ```no_run
/// # fn example(stream: impl futures::AsyncRead + futures::AsyncWrite + Unpin + 'static)
/// # -> capnp::Result<()> {
/// let driver = capnp_rpc::twoparty::ConnectionOptions::new().client(stream)?;
/// // Obtain driver.bootstrap::<your_schema::service::Client>(), then drive RPC.
/// # drop(driver);
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Copy, Default)]
#[must_use = "connection options take effect when a client or server is constructed"]
pub struct ConnectionOptions {
    driver: RpcDriver,
    reader: Option<ReaderOptions>,
}
impl ConnectionOptions {
    pub fn new() -> Self {
        Self::default()
    }

    /// Select legacy or bounded portable RPC, without a fallback.
    pub fn driver(mut self, driver: RpcDriver) -> Self {
        self.driver = driver;
        self
    }

    /// Override inbound reader limits for framing and protocol admission alike.
    /// Without this override, portable uses its configured reader limits.
    /// The override takes precedence regardless of builder call order.
    pub fn reader_options(mut self, reader: ReaderOptions) -> Self {
        self.reader = Some(reader);
        self
    }

    /// Construct a client with no local bootstrap capability.
    pub fn client(
        self,
        stream: impl AsyncRead + AsyncWrite + Unpin + 'static,
    ) -> capnp::Result<TwoPartyClient<'static>> {
        self.build(stream, None, VatId::Client)
    }

    /// Construct a server exposing a local bootstrap capability.
    pub fn server(
        self,
        stream: impl AsyncRead + AsyncWrite + Unpin + 'static,
        bootstrap: Client,
    ) -> capnp::Result<TwoPartyClient<'static>> {
        self.build(stream, Some(bootstrap), VatId::Server)
    }

    /// Use a caller-supplied monotonic elapsed-time clock for queue metrics.
    /// Required on hosts where `std::time::Instant` is unavailable, such as
    /// browsers. This clock is separate from the Unix-time clock used for TLS
    /// certificate authentication. It must not retain the stream or connection.
    pub fn client_with_clock(
        self,
        stream: impl AsyncRead + AsyncWrite + Unpin + 'static,
        clock: impl Fn() -> std::time::Duration + Send + Sync + 'static,
    ) -> capnp::Result<TwoPartyClient<'static>> {
        self.build_with_clock(stream, None, VatId::Client, clock)
    }

    /// Server variant of [`Self::client_with_clock`].
    pub fn server_with_clock(
        self,
        stream: impl AsyncRead + AsyncWrite + Unpin + 'static,
        bootstrap: Client,
        clock: impl Fn() -> std::time::Duration + Send + Sync + 'static,
    ) -> capnp::Result<TwoPartyClient<'static>> {
        self.build_with_clock(stream, Some(bootstrap), VatId::Server, clock)
    }

    fn protocol(self) -> (RpcDriver, ReaderOptions) {
        let reader = self.reader.unwrap_or_else(|| self.driver.reader_options());
        (self.driver.with_reader_options(reader), reader)
    }

    fn build(
        self,
        stream: impl AsyncRead + AsyncWrite + Unpin + 'static,
        bootstrap: Option<Client>,
        side: VatId,
    ) -> capnp::Result<TwoPartyClient<'static>> {
        let (driver, reader) = self.protocol();
        let (read, write) = stream.split();
        TwoPartyClient::from_network_with_driver(
            VatNetwork::new(read, write, side, reader),
            bootstrap,
            driver,
        )
    }

    fn build_with_clock(
        self,
        stream: impl AsyncRead + AsyncWrite + Unpin + 'static,
        bootstrap: Option<Client>,
        side: VatId,
        clock: impl Fn() -> std::time::Duration + Send + Sync + 'static,
    ) -> capnp::Result<TwoPartyClient<'static>> {
        let (driver, reader) = self.protocol();
        let (read, write) = stream.split();
        TwoPartyClient::from_network_with_driver(
            VatNetwork::new_with_clock(read, write, side, reader, clock),
            bootstrap,
            driver,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_reader_limits_win_in_either_builder_order() {
        let portable = crate::portable::Options::bounded(
            1.try_into().unwrap(),
            64.try_into().unwrap(),
            4096.try_into().unwrap(),
            8192.try_into().unwrap(),
        )
        .unwrap();
        let reader = ReaderOptions {
            traversal_limit_in_words: Some(128),
            nesting_limit: 7,
        };
        for options in [
            ConnectionOptions::new()
                .reader_options(reader)
                .driver(RpcDriver::Portable(portable)),
            ConnectionOptions::new()
                .driver(RpcDriver::Portable(portable))
                .reader_options(reader),
        ] {
            let (driver, framing) = options.protocol();
            let admission = driver.reader_options();
            assert_eq!(framing.traversal_limit_in_words, Some(128));
            assert_eq!(admission.traversal_limit_in_words, Some(128));
            assert_eq!(framing.nesting_limit, 7);
            assert_eq!(admission.nesting_limit, 7);
        }
        let (_, inherited) = ConnectionOptions::new()
            .driver(RpcDriver::Portable(portable))
            .protocol();
        assert_eq!(
            inherited.traversal_limit_in_words,
            portable.reader.traversal_limit_in_words
        );
        assert_eq!(inherited.nesting_limit, portable.reader.nesting_limit);
    }

    #[test]
    fn setup_adds_no_allocations_to_the_existing_construction_path() {
        let protocols = [
            RpcDriver::Legacy,
            RpcDriver::Portable(
                crate::portable::Options::bounded(
                    1.try_into().unwrap(),
                    64.try_into().unwrap(),
                    4096.try_into().unwrap(),
                    8192.try_into().unwrap(),
                )
                .unwrap(),
            ),
        ];
        for protocol in protocols {
            let direct = || {
                let (read, write) = futures::io::Cursor::new(Vec::<u8>::new()).split();
                drop(
                    TwoPartyClient::from_network_with_driver(
                        VatNetwork::new_with_clock(
                            read,
                            write,
                            VatId::Client,
                            protocol.reader_options(),
                            || std::time::Duration::ZERO,
                        ),
                        None,
                        protocol,
                    )
                    .unwrap(),
                );
            };
            let options = || {
                drop(
                    ConnectionOptions::new()
                        .driver(protocol)
                        .client_with_clock(futures::io::Cursor::new(Vec::<u8>::new()), || {
                            std::time::Duration::ZERO
                        })
                        .unwrap(),
                )
            };
            direct();
            options();
            let before = allocation_counter::measure(direct);
            let after = allocation_counter::measure(options);
            assert_eq!(after.count_total, before.count_total);
            assert_eq!(after.bytes_total, before.bytes_total);
        }
    }
}
