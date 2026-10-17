use crate::{cases, echo_capnp::workflows, support::VERSION, Result};
use capnp_rpc::{rpc_twoparty_capnp::Side, twoparty};
use tokio::{
    net::{TcpListener, TcpStream},
    time::timeout,
};
use tokio_util::compat::TokioAsyncReadCompatExt;

#[path = "../../rpc/capability-workflows.rs"]
mod capability;
#[path = "../../../src/rpc/io.rs"]
mod io;

fn implementation() -> Result<&'static str> {
    Ok("portable")
}

fn driver(
    socket: TcpStream,
    bootstrap: Option<capnp::capability::Client>,
    side: Side,
) -> Result<capnp_rpc::portable::RpcSystem<Side>> {
    let (read, write) = socket.into_split();
    crate::system(
        Box::new(twoparty::VatNetwork::new_with_send_buffer(
            read.compat(),
            io::TokioIo(write),
            side,
            Default::default(),
            |write| {
                socket2::SockRef::from(write.get_ref().as_ref())
                    .send_buffer_size()
                    .ok()
            },
        )),
        bootstrap,
    )
}
async fn connect(address: &str) -> Result<workflows::Client> {
    let socket = TcpStream::connect(address).await?;
    socket.set_nodelay(true)?;
    let mut driver = driver(socket, None, Side::Client)?;
    let client = driver.bootstrap(Side::Server);
    tokio::task::spawn_local(driver);
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
        tokio::task::spawn_local(async move {
            if let Err(error) = driver.await {
                eprintln!("workflow connection closed: {error}");
            }
        });
    }
}
pub fn run(args: Vec<String>) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(tokio::task::LocalSet::new().run_until(async move {
        tokio::task::spawn_local(async move {
            match args.first().map(String::as_str) {
                Some("runtime-info") => {
                    println!("tokio current_thread");
                    Ok(())
                }
                Some("workflow-server") if args.len() == 2 => serve(&args[1]).await,
                Some("workflow-measure") => capability::measure(&args[1..]).await,
                _ => Err("expected runtime-info, workflow-server or workflow-measure".into()),
            }
        })
        .await?
    }))
}
