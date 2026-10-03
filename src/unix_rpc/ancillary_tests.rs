use super::*;
use std::{io::Read, os::fd::IntoRawFd};

fn witnessed() -> (Rc<OwnedFd>, std::os::unix::net::UnixStream) {
    let (passed, peer) = std::os::unix::net::UnixStream::pair().unwrap();
    peer.set_nonblocking(true).unwrap();
    (Rc::new(passed.into()), peer)
}
fn closed(peer: &mut std::os::unix::net::UnixStream) {
    assert_eq!(peer.read(&mut [0]).unwrap(), 0);
}

#[tokio::test(flavor = "current_thread")]
async fn fallback_cloexec_and_full_control_buffer_close_all_excess_fds() {
    for limit in [0, 1, 2] {
        let (sender, receiver) = UnixStream::pair().unwrap();
        let mut peers = Vec::new();
        let fds = (0..3)
            .map(|_| {
                let (fd, peer) = witnessed();
                peers.push(peer);
                fd
            })
            .collect::<Vec<_>>();
        super::super::write_message(&sender, b"x", &fds)
            .await
            .unwrap();
        drop(fds);
        receiver.readable().await.unwrap();
        let mut pending = PendingFds::default();
        assert_eq!(
            recv_with_policy(&receiver, &mut [0; 1], &mut pending, limit, true).unwrap(),
            1
        );
        assert_eq!(pending.len(), limit);
        for fd in pending.slots.iter().flatten() {
            let flags = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFD) };
            assert!(flags >= 0);
            assert_ne!(flags & libc::FD_CLOEXEC, 0);
        }
        for peer in peers.iter_mut().skip(limit) {
            closed(peer);
        }
        drop(pending);
        for peer in &mut peers {
            closed(peer);
        }
    }
}

// An initialized synthetic control message owns genuine duplicated descriptors,
// just as a successful recvmsg does. The collector must adopt every one.
fn control(fds: Vec<OwnedFd>, run: impl FnOnce(&libc::msghdr)) {
    let mut buffer = control_buffer(fds.len());
    unsafe {
        let mut msg: libc::msghdr = mem::zeroed();
        msg.msg_control = buffer.as_mut_ptr().cast();
        msg.msg_controllen =
            libc::CMSG_SPACE((fds.len() * mem::size_of::<libc::c_int>()) as u32) as _;
        let header = libc::CMSG_FIRSTHDR(&msg);
        (*header).cmsg_level = libc::SOL_SOCKET;
        (*header).cmsg_type = libc::SCM_RIGHTS;
        (*header).cmsg_len =
            libc::CMSG_LEN((fds.len() * mem::size_of::<libc::c_int>()) as u32) as _;
        let data = libc::CMSG_DATA(header).cast::<libc::c_int>();
        for (i, fd) in fds.into_iter().enumerate() {
            data.add(i).write_unaligned(fd.into_raw_fd());
        }
        run(&msg);
    }
}

#[test]
fn configuration_failure_closes_earlier_and_later_received_descriptors() {
    let mut peers = Vec::new();
    let fds = (0..3)
        .map(|_| {
            let (fd, peer) = witnessed();
            peers.push(peer);
            Rc::try_unwrap(fd).unwrap()
        })
        .collect();
    let mut pending = PendingFds::default();
    control(fds, |msg| unsafe {
        let mut configured = 0;
        assert!(collect_fds(msg, &mut pending, 3, |_| {
            configured += 1;
            if configured == 2 {
                Err(io::Error::other("injected fcntl failure"))
            } else {
                Ok(())
            }
        })
        .is_err());
        assert_eq!(configured, 2);
    });
    assert!(pending.is_empty());
    for peer in &mut peers {
        closed(peer);
    }
}

#[test]
fn truncated_control_length_never_reads_beyond_returned_bytes() {
    let (fd, mut peer) = witnessed();
    control(vec![Rc::try_unwrap(fd).unwrap()], |msg| unsafe {
        let mut truncated = *msg;
        // Darwin may leave cmsg_len larger than msg_controllen on truncation.
        let header = libc::CMSG_FIRSTHDR(&truncated);
        (*header).cmsg_len = libc::CMSG_LEN(3 * mem::size_of::<libc::c_int>() as u32) as _;
        truncated.msg_controllen = libc::CMSG_LEN(mem::size_of::<libc::c_int>() as u32) as _;
        truncated.msg_flags = libc::MSG_CTRUNC;
        let mut pending = PendingFds::default();
        collect_fds(&truncated, &mut pending, 1, set_cloexec).unwrap();
        assert_eq!(pending.len(), 1);
    });
    closed(&mut peer);
}

#[tokio::test(flavor = "current_thread")]
async fn closed_peer_write_reports_error_with_default_sigpipe_handler() {
    const MARKER: &str = "REPROTO_SIGPIPE_CHILD";
    if std::env::var_os(MARKER).is_none() {
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["unix_rpc::ancillary::tests::closed_peer_write_reports_error_with_default_sigpipe_handler", "--exact"])
            .env(MARKER, "1").status().unwrap();
        assert!(
            status.success(),
            "send terminated the isolated child: {status}"
        );
        return;
    }
    // Isolated test process: no process-wide signal change in the parent suite.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    let (sender, receiver) = UnixStream::pair().unwrap();
    // Exercise the per-send flag even if the socket constructor suppressed
    // SIGPIPE itself. Configure while connected; Darwin rejects options after
    // the peer closes. The process-wide handler above remains SIG_DFL.
    #[cfg(target_os = "macos")]
    unsafe {
        let disabled: libc::c_int = 0;
        assert_eq!(
            libc::setsockopt(
                sender.as_raw_fd(),
                libc::SOL_SOCKET,
                libc::SO_NOSIGPIPE,
                (&disabled as *const libc::c_int).cast(),
                mem::size_of_val(&disabled) as _,
            ),
            0,
            "could not enable SIGPIPE for the regression: {}",
            io::Error::last_os_error()
        );
    }
    drop(receiver);
    let error = super::super::write_message(&sender, b"x", &[])
        .await
        .unwrap_err();
    assert!(
        matches!(
            error.kind(),
            io::ErrorKind::BrokenPipe
                | io::ErrorKind::ConnectionReset
                | io::ErrorKind::NotConnected
        ),
        "unexpected closed-peer error: {error:?}"
    );
}
