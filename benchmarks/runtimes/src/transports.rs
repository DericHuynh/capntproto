use crate::{measure, Args, Result};
use capntproto_transport::rustls;
use futures::{AsyncRead, AsyncWrite};
use std::{
    io,
    pin::Pin,
    task::{Context, Poll},
};
use tokio::io::{AsyncRead as TokioRead, AsyncWrite as TokioWrite, ReadBuf};
use tokio_util::compat::TokioAsyncReadCompatExt;

struct Socket<S>(S);
impl<S: TokioRead + Unpin> TokioRead for Socket<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let before = buffer.filled().len();
        let result = Pin::new(&mut self.0).poll_read(cx, buffer);
        if matches!(result, Poll::Ready(Ok(()))) {
            measure::read(buffer.filled().len() - before);
        }
        result
    }
}
impl<S: TokioWrite + Unpin> TokioWrite for Socket<S> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        let result = Pin::new(&mut self.0).poll_write(cx, bytes);
        if let Poll::Ready(Ok(n)) = &result {
            measure::write(*n);
        }
        result
    }
    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        let result = Pin::new(&mut self.0).poll_write_vectored(cx, bytes);
        if let Poll::Ready(Ok(n)) = &result {
            measure::write(*n);
        }
        result
    }
    fn is_write_vectored(&self) -> bool {
        self.0.is_write_vectored()
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_shutdown(cx)
    }
}
#[cfg(feature = "webtransport")]
struct Stream<S>(S);
#[cfg(feature = "webtransport")]
impl<S: AsyncRead + Unpin> AsyncRead for Stream<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        let result = Pin::new(&mut self.0).poll_read(cx, buffer);
        if let Poll::Ready(Ok(n)) = &result {
            measure::read(*n);
        }
        result
    }
}
#[cfg(feature = "webtransport")]
impl<S: AsyncWrite + Unpin> AsyncWrite for Stream<S> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        let result = Pin::new(&mut self.0).poll_write(cx, bytes);
        if let Poll::Ready(Ok(n)) = &result {
            measure::write(*n);
        }
        result
    }
    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        let result = Pin::new(&mut self.0).poll_write_vectored(cx, bytes);
        if let Poll::Ready(Ok(n)) = &result {
            measure::write(*n);
        }
        result
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_flush(cx)
    }
    fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0).poll_close(cx)
    }
}

pub fn run(args: &Args) -> Result<measure::Sample> {
    match args.runtime.as_str() {
        "tokio" | "tokio-driven" => tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?
            .block_on(tokio::task::LocalSet::new().run_until(async {
                tokio::time::timeout(std::time::Duration::from_secs(120), run_tokio(args)).await?
            })),
        #[cfg(all(feature = "monoio", target_os = "linux"))]
        "monoio-uring" => monoio::RuntimeBuilder::<monoio::IoUringDriver>::new()
            .enable_timer()
            .build()?
            .block_on(completion::run(args)),
        #[cfg(feature = "monoio")]
        "monoio-epoll" => monoio::RuntimeBuilder::<monoio::LegacyDriver>::new()
            .enable_timer()
            .build()?
            .block_on(completion::run(args)),
        _ => Err(format!("runtime not supported in this build: {}", args.runtime).into()),
    }
}
async fn run_tokio(args: &Args) -> Result<measure::Sample> {
    match args.transport.as_str() {
        "tls" if args.runtime == "tokio" => tls_tokio(args).await,
        #[cfg(feature = "driven")]
        "tls" => tls_driven(args).await,
        #[cfg(feature = "webtransport")]
        "webtransport" if args.runtime == "tokio" => wt_tokio(args).await,
        #[cfg(all(feature = "webtransport", feature = "driven"))]
        "webtransport" => wt_driven(args).await,
        _ => Err("transport not enabled".into()),
    }
}
async fn tls_tokio(args: &Args) -> Result<measure::Sample> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let config = args.server_config()?;
    let client = args.client_config()?;
    let serving = async {
        let (socket, _) = listener.accept().await?;
        socket.set_nodelay(true)?;
        let tls = rustls::TlsStream::accept(Socket(socket), config).await?;
        crate::serve(tls.compat(), args).await
    };
    let calling = async {
        let socket = tokio::net::TcpStream::connect(address).await?;
        socket.set_nodelay(true)?;
        let tls =
            rustls::TlsStream::connect(Socket(socket), client, "localhost".try_into()?).await?;
        crate::call(tls.compat(), args).await
    };
    let (server, client) = futures::join!(serving, calling);
    server?;
    client
}
#[cfg(feature = "driven")]
fn transport_error<E: std::fmt::Debug>(
    error: capntproto_transport::driven::Error<E>,
) -> Box<dyn std::error::Error> {
    io::Error::other(format!("transport driver: {error:?}")).into()
}
#[cfg(feature = "driven")]
async fn tls_driven(args: &Args) -> Result<measure::Sample> {
    use capntproto_transport::{
        driven::DriverExt,
        tokio::{Reader, Writer},
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let config = args.server_config()?;
    let client = args.client_config()?;
    let serving = async {
        let (socket, _) = listener.accept().await?;
        socket.set_nodelay(true)?;
        let (reader, writer) = socket.into_split();
        let (mut stream, driver) =
            rustls::server(Reader(Socket(reader)), Writer(Socket(writer)), config)?;
        driver
            .run_until(async {
                stream.ready().await?;
                crate::serve(stream, args).await
            })
            .await
            .map_err(transport_error)?
    };
    let calling = async {
        let socket = tokio::net::TcpStream::connect(address).await?;
        socket.set_nodelay(true)?;
        let (reader, writer) = socket.into_split();
        let (mut stream, driver) = rustls::client(
            Reader(Socket(reader)),
            Writer(Socket(writer)),
            client,
            "localhost".try_into()?,
        )?;
        driver
            .run_until(async {
                stream.ready().await?;
                crate::call(stream, args).await
            })
            .await
            .map_err(transport_error)?
    };
    let (server, client) = futures::join!(serving, calling);
    server?;
    client
}
#[cfg(feature = "webtransport")]
async fn wt_tokio(args: &Args) -> Result<measure::Sample> {
    use capntproto_transport::{webtransport as wt, SystemClock};
    use std::sync::Arc;
    let certificate = std::fs::read(args.certificates.join("server.der"))?;
    let key = wt::tls::pki_types::PrivateKeyDer::try_from(std::fs::read(
        args.certificates.join("server-key.der"),
    )?)
    .map_err(io::Error::other)?;
    let mut roots = wt::tls::RootCertStore::empty();
    roots.add(certificate.clone().into())?;
    let server = wt::Server::bind(
        "127.0.0.1:0".parse()?,
        vec![certificate.into()],
        key,
        Arc::new(SystemClock),
    )?;
    let client = wt::Client::new(roots, Arc::new(SystemClock))?;
    let url = format!("https://127.0.0.1:{}/rpc", server.local_addr()?.port());
    let serving = async {
        let mut session = server.accept().await?.accept().await?;
        crate::serve(Stream(session.accept_bi().await?), args).await
    };
    let calling = async {
        let mut session = client.connect(&url).await?;
        crate::call(Stream(session.open_bi().await?), args).await
    };
    let (server, client) = futures::join!(serving, calling);
    server?;
    client
}
#[cfg(all(feature = "webtransport", feature = "driven"))]
async fn wt_driven(args: &Args) -> Result<measure::Sample> {
    use capntproto_transport::{tokio::webtransport as wt, SystemClock};
    use std::sync::Arc;
    let certificate = std::fs::read(args.certificates.join("server.der"))?;
    let key = wt::tls::pki_types::PrivateKeyDer::try_from(std::fs::read(
        args.certificates.join("server-key.der"),
    )?)
    .map_err(io::Error::other)?;
    let mut roots = wt::tls::RootCertStore::empty();
    roots.add(certificate.clone().into())?;
    let (server, sd) = wt::server_driven(
        "127.0.0.1:0".parse()?,
        vec![certificate.into()],
        key,
        Arc::new(SystemClock),
    )?;
    let (client, cd) = wt::client_driven(roots, Arc::new(SystemClock))?;
    wt_pair(server, sd, client, cd, args).await
}
#[cfg(all(feature = "webtransport", feature = "driven"))]
async fn wt_pair<
    S: capntproto_transport::driven::Driver<Error = io::Error>,
    C: capntproto_transport::driven::Driver<Error = io::Error>,
>(
    server: capntproto_transport::webtransport::driven::Server,
    sd: S,
    client: capntproto_transport::webtransport::driven::Client,
    cd: C,
    args: &Args,
) -> Result<measure::Sample> {
    use capntproto_transport::driven::DriverExt;
    let address = server.local_addr()?;
    let url = format!("https://127.0.0.1:{}/rpc", address.port());
    let serving = sd.run_until(async move {
        let mut session = server.accept().await?.accept().await?;
        crate::serve(Stream(session.accept_bi().await?), args).await
    });
    let calling = cd.run_until(async move {
        let mut session = client.connect(address, &url).await?;
        crate::call(Stream(session.open_bi().await?), args).await
    });
    let (server, client) = futures::join!(serving, calling);
    server.map_err(transport_error)??;
    client.map_err(transport_error)?
}

#[cfg(feature = "monoio")]
mod completion {
    use super::*;
    use capntproto_transport::{
        driven::DriverExt,
        io::{Close, Completion, Read, Write},
        monoio::{Reader, Writer},
    };
    use monoio::io::Splitable;
    struct CountedRead<R>(R);
    struct CountedWrite<W>(W);
    impl<R: Read> Read for CountedRead<R> {
        type Buffer = R::Buffer;
        type Error = R::Error;
        async fn read(&mut self, buffer: R::Buffer) -> Completion<R::Buffer, R::Error> {
            let (result, buffer) = self.0.read(buffer).await;
            if let Ok(n) = &result {
                measure::read(*n);
            }
            (result, buffer)
        }
    }
    impl<W: Write> Write for CountedWrite<W> {
        type Buffer = W::Buffer;
        type Error = W::Error;
        async fn write(&mut self, buffer: W::Buffer) -> Completion<W::Buffer, W::Error> {
            let (result, buffer) = self.0.write(buffer).await;
            if let Ok(n) = &result {
                measure::write(*n);
            }
            (result, buffer)
        }
        async fn flush(&mut self) -> std::result::Result<(), W::Error> {
            self.0.flush().await
        }
    }
    impl<W: Close> Close for CountedWrite<W> {
        async fn close(&mut self) -> std::result::Result<(), W::Error> {
            self.0.close().await
        }
    }
    pub async fn run(args: &Args) -> Result<measure::Sample> {
        match args.transport.as_str() {
            "tls" => tls(args).await,
            #[cfg(feature = "webtransport")]
            "webtransport" => wt(args).await,
            _ => Err("unsupported transport".into()),
        }
    }
    async fn tls(args: &Args) -> Result<measure::Sample> {
        let listener = monoio::net::TcpListener::bind("127.0.0.1:0")?;
        let address = listener.local_addr()?;
        let config = args.server_config()?;
        let client = args.client_config()?;
        let serving = async {
            let (socket, _) = listener.accept().await?;
            socket.set_nodelay(true)?;
            let (reader, writer) = socket.into_split();
            let (mut stream, driver) = rustls::server(
                CountedRead(Reader(reader)),
                CountedWrite(Writer(writer)),
                config,
            )?;
            driver
                .run_until(async {
                    stream.ready().await?;
                    crate::serve(stream, args).await
                })
                .await
                .map_err(transport_error)?
        };
        let calling = async {
            let socket = monoio::net::TcpStream::connect(address).await?;
            socket.set_nodelay(true)?;
            let (reader, writer) = socket.into_split();
            let (mut stream, driver) = rustls::client(
                CountedRead(Reader(reader)),
                CountedWrite(Writer(writer)),
                client,
                "localhost".try_into()?,
            )?;
            driver
                .run_until(async {
                    stream.ready().await?;
                    crate::call(stream, args).await
                })
                .await
                .map_err(transport_error)?
        };
        let (server, client) = futures::join!(serving, calling);
        server?;
        client
    }
    #[cfg(feature = "webtransport")]
    async fn wt(args: &Args) -> Result<measure::Sample> {
        use capntproto_transport::{monoio::webtransport as wt, SystemClock};
        use std::sync::Arc;
        let certificate = std::fs::read(args.certificates.join("server.der"))?;
        let key = wt::tls::pki_types::PrivateKeyDer::try_from(std::fs::read(
            args.certificates.join("server-key.der"),
        )?)
        .map_err(io::Error::other)?;
        let mut roots = wt::tls::RootCertStore::empty();
        roots.add(certificate.clone().into())?;
        let (server, sd) = wt::server_driven(
            "127.0.0.1:0".parse()?,
            vec![certificate.into()],
            key,
            Arc::new(SystemClock),
        )?;
        let (client, cd) = wt::client_driven(roots, Arc::new(SystemClock))?;
        wt_pair(server, sd, client, cd, args).await
    }
}
