//! Bidirectional Linux SCM_RIGHTS interoperability with a real C++ RPC runtime.
#[cfg(target_os = "linux")]
#[tokio::main(flavor = "current_thread")]
async fn main() {
    use capnp_rpc::{rpc_twoparty_capnp::Side, RpcSystem};
    use capntproto_test_support::runtime_test_capnp::harness;
    use std::{
        io::Write,
        os::{fd::OwnedFd, unix::fs::FileExt},
        process::{Child, Command},
        rc::Rc,
    };
    struct Peer(Child);
    impl Drop for Peer {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    struct Dummy;
    impl harness::Server for Dummy {}
    fn value(fd: Option<Rc<OwnedFd>>) -> u8 {
        let file = std::fs::File::from(
            fd.expect("missing SCM_RIGHTS descriptor")
                .try_clone()
                .unwrap(),
        );
        let mut byte = [0];
        file.read_exact_at(&mut byte, 0).unwrap();
        byte[0]
    }
    let executable = std::env::args().nth(1).expect("C++ FD peer executable");
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("rpc.sock");
    let listener = tokio::net::UnixListener::bind(&path).unwrap();
    let _peer = Peer(Command::new(executable).arg(path).spawn().unwrap());
    tokio::task::LocalSet::new().run_until(async {
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            let (socket, _) = listener.accept().await.unwrap();
            let network = capntproto::unix_rpc::VatNetwork::new(socket, Side::Client, Default::default());
            let mut system = RpcSystem::new(Box::new(network), None);
            let remote: harness::Client = system.bootstrap(Side::Server);
            let disconnect = system.get_disconnector();
            let task = tokio::task::spawn_local(system);
            assert_eq!(value(remote.client.get_fd().await.unwrap()), 9);
            assert_eq!(remote.echo_request().send().promise.await.unwrap().get().unwrap().get_value(),42);
            let mut file = tempfile::tempfile().unwrap(); file.write_all(&[7]).unwrap();
            let local: harness::Client = capnp_rpc::new_fd_client(Dummy, file.into());
            let mut request = remote.bounce_request(); request.get().set_cap(local.clone());
            let response = request.send().promise.await.unwrap();
            assert_eq!(value(response.get().unwrap().get_cap().unwrap().client.get_fd().await.unwrap()),10);
            let mut request = remote.fd_caps_request(); request.get().init_caps(1).set(0, local.client.hook.add_ref());
            let response = request.send().promise.await.unwrap();
            let reflected = capnp::capability::get_resolved_cap(response.get().unwrap().get_caps().unwrap().get(0).unwrap()).await;
            assert_eq!(reflected.client.hook.get_ptr(), local.client.hook.get_ptr());
            assert_eq!(value(reflected.client.get_fd().await.unwrap()),7);
            disconnect.await.unwrap(); task.abort();
            println!("C++ FD interop: SCM_RIGHTS bootstrap, bidirectional calls/returns, getFd and reflected capability identity passed");
        }).await.unwrap();
    }).await;
}
#[cfg(not(target_os = "linux"))]
fn main() {
    panic!("Linux SCM_RIGHTS transport required");
}
