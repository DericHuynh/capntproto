//! Reader and diagnostic configuration for the single RPC implementation.
use super::*;

/// Configuration for an already connected byte stream. Both constructors
/// return a driver owned by the caller; they do not spawn a background task.
#[derive(Clone, Copy, Default)]
#[must_use = "connection options take effect when a client or server is constructed"]
pub struct ConnectionOptions {
    reader: ReaderOptions,
}
impl ConnectionOptions {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn reader_options(mut self, reader: ReaderOptions) -> Self {
        self.reader = reader;
        self
    }
    pub fn client(
        self,
        stream: impl AsyncRead + AsyncWrite + Unpin + 'static,
    ) -> capnp::Result<TwoPartyClient<'static>> {
        self.build(stream, None, VatId::Client)
    }
    pub fn server(
        self,
        stream: impl AsyncRead + AsyncWrite + Unpin + 'static,
        bootstrap: Client,
    ) -> capnp::Result<TwoPartyClient<'static>> {
        self.build(stream, Some(bootstrap), VatId::Server)
    }
    pub fn client_with_clock(
        self,
        stream: impl AsyncRead + AsyncWrite + Unpin + 'static,
        clock: impl Fn() -> std::time::Duration + Send + Sync + 'static,
    ) -> capnp::Result<TwoPartyClient<'static>> {
        let (read, write) = stream.split();
        Ok(TwoPartyClient::from_network(
            VatNetwork::new_with_clock(read, write, VatId::Client, self.reader, clock),
            None,
        ))
    }
    pub fn server_with_clock(
        self,
        stream: impl AsyncRead + AsyncWrite + Unpin + 'static,
        bootstrap: Client,
        clock: impl Fn() -> std::time::Duration + Send + Sync + 'static,
    ) -> capnp::Result<TwoPartyClient<'static>> {
        let (read, write) = stream.split();
        Ok(TwoPartyClient::from_network(
            VatNetwork::new_with_clock(read, write, VatId::Server, self.reader, clock),
            Some(bootstrap),
        ))
    }
    fn build(
        self,
        stream: impl AsyncRead + AsyncWrite + Unpin + 'static,
        bootstrap: Option<Client>,
        side: VatId,
    ) -> capnp::Result<TwoPartyClient<'static>> {
        let (read, write) = stream.split();
        Ok(TwoPartyClient::from_network(
            VatNetwork::new(read, write, side, self.reader),
            bootstrap,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_reader_limits_are_preserved() {
        let options = ConnectionOptions::new().reader_options(ReaderOptions {
            traversal_limit_in_words: Some(128),
            nesting_limit: 7,
        });
        assert_eq!(options.reader.traversal_limit_in_words, Some(128));
        assert_eq!(options.reader.nesting_limit, 7);
    }
    #[test]
    fn setup_adds_no_allocations_to_direct_construction() {
        let direct = || {
            let (read, write) = futures::io::Cursor::new(Vec::<u8>::new()).split();
            drop(TwoPartyClient::from_network(
                VatNetwork::new_with_clock(
                    read,
                    write,
                    VatId::Client,
                    ReaderOptions::new(),
                    || std::time::Duration::ZERO,
                ),
                None,
            ));
        };
        let configured = || {
            drop(
                ConnectionOptions::new()
                    .client_with_clock(futures::io::Cursor::new(Vec::<u8>::new()), || {
                        std::time::Duration::ZERO
                    })
                    .unwrap(),
            );
        };
        direct();
        configured();
        let before = allocation_counter::measure(direct);
        let after = allocation_counter::measure(configured);
        assert_eq!(after.count_total, before.count_total);
        assert_eq!(after.bytes_total, before.bytes_total);
    }
}
