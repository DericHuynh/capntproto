use crate::{cases, echo_capnp::workflows, support::VERSION, Result};
use capnp_rpc::{rpc_twoparty_capnp::Side, twoparty, VatNetwork};
use compio::{
    net::{TcpListener, TcpStream},
    runtime::time::timeout,
};

#[path = "../../rpc/capability-workflows.rs"]
mod capability;

fn implementation() -> Result<&'static str> {
    Ok("portable")
}

fn driver(
    socket: TcpStream,
    bootstrap: Option<capnp::capability::Client>,
    side: Side,
) -> Result<capnp_rpc::portable::RpcSystem<Side>> {
    let completion =
        std::env::var("CAPNTPROTO_BENCH_RUNTIME").as_deref() == Ok("compio-completion");
    let network: Box<dyn VatNetwork<Side>> = if completion {
        // This compatibility path owns operation buffers and copies across the
        // borrowed futures-io boundary. Its costs are measured explicitly.
        let probe = socket.clone();
        let (read, write) = socket.into_split();
        Box::new(twoparty::VatNetwork::new_with_send_buffer(
            Box::pin(compio::io::compat::AsyncReadStream::new(read)),
            Box::pin(compio::io::compat::AsyncWriteStream::new(write)),
            side,
            Default::default(),
            move |_| socket2::SockRef::from(&probe).send_buffer_size().ok(),
        ))
    } else {
        // PollFd preserves borrowed reads and scatter/gather writes. It uses
        // Compio's executor and readiness registration, with no Tokio reactor.
        socket2::SockRef::from(&socket).set_nonblocking(true)?;
        let (read, write) = socket.into_split();
        Box::new(twoparty::VatNetwork::new_with_send_buffer(
            read.into_poll_fd()?,
            write.into_poll_fd()?,
            side,
            Default::default(),
            |write| socket2::SockRef::from(write).send_buffer_size().ok(),
        ))
    };
    crate::system(network, bootstrap)
}
async fn connect(address: &str) -> Result<workflows::Client> {
    let socket = TcpStream::connect(address).await?;
    socket.set_nodelay(true)?;
    let mut driver = driver(socket, None, Side::Client)?;
    let client = driver.bootstrap(Side::Server);
    compio::runtime::spawn(driver).detach();
    Ok(client)
}
async fn serve(address: &str) -> Result<()> {
    let listener = TcpListener::bind(address).await?;
    crate::ready(listener.local_addr()?)?;
    loop {
        let (socket, _) = listener.accept().await?;
        socket.set_nodelay(true)?;
        let bootstrap: workflows::Client = capnp_rpc::new_client(capability::Service);
        let driver = driver(socket, Some(bootstrap.client), Side::Server)?;
        compio::runtime::spawn(async move {
            if let Err(error) = driver.await {
                eprintln!("workflow connection closed: {error}");
            }
        })
        .detach();
    }
}
pub fn run(args: Vec<String>) -> Result<()> {
    let mut proactor = compio::driver::ProactorBuilder::new();
    match std::env::var("CAPNTPROTO_COMPIO_DRIVER")
        .as_deref()
        .unwrap_or("auto")
    {
        "auto" => {}
        "poll" => {
            proactor.driver_type(compio::driver::DriverType::Poll);
        }
        "io-uring" => {
            proactor.driver_type(compio::driver::DriverType::IoUring);
        }
        _ => return Err("CAPNTPROTO_COMPIO_DRIVER must be auto, poll or io-uring".into()),
    }
    let runtime = compio::runtime::Runtime::builder()
        .with_proactor(proactor)
        .build()?;
    eprintln!("compio driver: {:?}", runtime.driver_type());
    runtime.block_on(async move {
        // Match the Tokio benchmark's separately scheduled root task.
        compio::runtime::spawn(async move {
            match args.first().map(String::as_str) {
                Some("runtime-info") => {
                    println!(
                        "compio {:?}",
                        compio::runtime::Runtime::current().driver_type()
                    );
                    Ok(())
                }
                Some("workflow-server") if args.len() == 2 => serve(&args[1]).await,
                Some("workflow-measure") => capability::measure(&args[1..]).await,
                _ => Err("expected runtime-info, workflow-server or workflow-measure".into()),
            }
        })
        .await?
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::{AsyncReadExt, AsyncWriteExt};
    use std::{io::IoSlice, time::Duration};

    #[test]
    fn adapters_preserve_cancelled_reads_vectored_bytes_and_half_close() {
        let mut proactor = compio::driver::ProactorBuilder::new();
        proactor.driver_type(compio::driver::DriverType::Poll);
        let runtime = compio::runtime::Runtime::builder()
            .with_proactor(proactor)
            .build()
            .unwrap();
        runtime.block_on(async {
            for completion in [false, true] {
                timeout(Duration::from_secs(5), async {
                    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
                    let (a, (b, _)) = futures::try_join!(
                        TcpStream::connect(listener.local_addr().unwrap()),
                        listener.accept()
                    )
                    .unwrap();
                    let (mut read, mut write): (
                        Box<dyn futures::AsyncRead + Unpin>,
                        Box<dyn futures::AsyncWrite + Unpin>,
                    ) = if completion {
                        (
                            Box::new(Box::pin(compio::io::compat::AsyncReadStream::new(a))),
                            Box::new(Box::pin(compio::io::compat::AsyncWriteStream::new(b))),
                        )
                    } else {
                        socket2::SockRef::from(&a).set_nonblocking(true).unwrap();
                        socket2::SockRef::from(&b).set_nonblocking(true).unwrap();
                        (
                            Box::new(a.into_poll_fd().unwrap()),
                            Box::new(b.into_poll_fd().unwrap()),
                        )
                    };
                    let mut abandoned = [0; 16];
                    {
                        let pending = read.read(&mut abandoned);
                        futures::pin_mut!(pending);
                        assert!(futures::poll!(pending).is_pending());
                    }
                    let slices = [IoSlice::new(b"abc"), IoSlice::new(b"defg")];
                    let written = write.write_vectored(&slices).await.unwrap();
                    assert!((1..=7).contains(&written));
                    write.write_all(&b"abcdefg"[written..]).await.unwrap();
                    write.flush().await.unwrap();
                    write.close().await.unwrap();
                    let mut bytes = Vec::new();
                    read.read_to_end(&mut bytes).await.unwrap();
                    assert_eq!(bytes, b"abcdefg");
                    assert_eq!(abandoned, [0; 16]);
                })
                .await
                .unwrap();
            }
        });
    }
}
