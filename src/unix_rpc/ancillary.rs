//! Platform-specific SCM_RIGHTS I/O. Framing and descriptor ownership live in
//! the parent module; all foreign control-buffer traversal stays here.
use super::PendingFds;
use std::{
    io, mem,
    os::fd::{AsRawFd, FromRawFd, OwnedFd},
    rc::Rc,
};
use tokio::net::UnixStream;

// Linux and supported macOS releases suppress SIGPIPE per send. Setting
// SO_NOSIGPIPE during a write is too late on Darwin: a closed peer can make
// setsockopt fail with EINVAL before sendmsg reports the actual disconnect.
const SEND_FLAGS: libc::c_int = libc::MSG_NOSIGNAL;
#[cfg(target_os = "linux")]
const RECV_FLAGS: libc::c_int = libc::MSG_CMSG_CLOEXEC;
#[cfg(target_os = "macos")]
const RECV_FLAGS: libc::c_int = 0;

// Ancillary storage is aligned for cmsghdr. Its initialized length is the full
// capacity passed to the kernel; no uninitialized padding is ever read.
fn control_buffer(count: usize) -> Vec<usize> {
    if count == 0 {
        return vec![];
    }
    // SAFETY: count is bounded to 253 before callers reach this function.
    let bytes =
        unsafe { libc::CMSG_SPACE((count * mem::size_of::<libc::c_int>()) as u32) } as usize;
    vec![0; bytes.div_ceil(mem::size_of::<usize>())]
}
pub(super) fn send_once(
    socket: &UnixStream,
    bytes: &[u8],
    fds: &[Rc<OwnedFd>],
) -> io::Result<usize> {
    if fds.len() > 253 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "too many outgoing descriptors",
        ));
    }
    let mut control = control_buffer(fds.len());
    let mut iov = libc::iovec {
        iov_base: bytes.as_ptr().cast_mut().cast(),
        iov_len: bytes.len(),
    };
    // SAFETY: zero is a valid empty msghdr; pointers refer to live, properly
    // aligned buffers for the entire synchronous sendmsg call.
    unsafe {
        let mut msg: libc::msghdr = mem::zeroed();
        msg.msg_iov = &mut iov;
        msg.msg_iovlen = 1;
        if !fds.is_empty() {
            msg.msg_control = control.as_mut_ptr().cast();
            msg.msg_controllen =
                libc::CMSG_SPACE((fds.len() * mem::size_of::<libc::c_int>()) as u32) as _;
            let header = libc::CMSG_FIRSTHDR(&msg);
            (*header).cmsg_level = libc::SOL_SOCKET;
            (*header).cmsg_type = libc::SCM_RIGHTS;
            (*header).cmsg_len =
                libc::CMSG_LEN((fds.len() * mem::size_of::<libc::c_int>()) as u32) as _;
            let data = libc::CMSG_DATA(header).cast::<libc::c_int>();
            for (i, fd) in fds.iter().enumerate() {
                data.add(i).write(fd.as_raw_fd());
            }
        }
        let count = libc::sendmsg(socket.as_raw_fd(), &msg, SEND_FLAGS);
        if count < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(count as usize)
        }
    }
}

fn set_cloexec(fd: &OwnedFd) -> io::Result<()> {
    // SAFETY: fd is live and owned throughout both fcntl calls.
    unsafe {
        let flags = libc::fcntl(fd.as_raw_fd(), libc::F_GETFD);
        if flags < 0 || libc::fcntl(fd.as_raw_fd(), libc::F_SETFD, flags | libc::FD_CLOEXEC) < 0 {
            Err(io::Error::last_os_error())
        } else {
            Ok(())
        }
    }
}

// SAFETY: msg must refer to a live, initialized, aligned control buffer of at
// least msg_controllen bytes, containing each newly received descriptor once.
// It must not be processed again: this consumes ownership of every rights entry.
#[allow(
    clippy::unnecessary_cast,
    reason = "Darwin control lengths are u32, Linux uses usize."
)]
unsafe fn collect_fds(
    msg: &libc::msghdr,
    fds: &mut PendingFds,
    max_fds: usize,
    mut configure: impl FnMut(&OwnedFd) -> io::Result<()>,
) -> io::Result<()> {
    let base = msg.msg_control as usize;
    let used = msg.msg_controllen as usize;
    let minimum = libc::CMSG_LEN(0) as usize;
    let mut header = libc::CMSG_FIRSTHDR(msg);
    let mut error = None;
    while !header.is_null() {
        let offset = (header as usize).saturating_sub(base);
        let remaining = used.saturating_sub(offset);
        if remaining < minimum {
            break;
        }
        let claimed = (*header).cmsg_len as usize;
        if claimed < minimum {
            error = Some(io::Error::new(
                io::ErrorKind::InvalidData,
                "short ancillary header",
            ));
            break;
        }
        // Darwin can retain the original cmsg_len after truncation. Never read
        // beyond the actual returned buffer, even when the header claims more.
        let available = claimed.min(remaining);
        if (*header).cmsg_level == libc::SOL_SOCKET && (*header).cmsg_type == libc::SCM_RIGHTS {
            let count = (available - minimum) / mem::size_of::<libc::c_int>();
            let data = libc::CMSG_DATA(header).cast::<libc::c_int>();
            for i in 0..count {
                let fd = OwnedFd::from_raw_fd(data.add(i).read_unaligned());
                if error.is_none() && fds.len() < max_fds {
                    match configure(&fd) {
                        Ok(()) => fds.push(fd),
                        Err(e) => error = Some(e),
                    }
                }
                // Extras and failed configurations drop here. Continue through
                // every entry after failure so no kernel-installed FD leaks.
            }
        }
        if claimed > remaining {
            break;
        }
        header = libc::CMSG_NXTHDR(msg, header);
    }
    if let Some(error) = error {
        fds.clear();
        Err(error)
    } else {
        Ok(())
    }
}

pub(super) fn recv_once(
    socket: &UnixStream,
    bytes: &mut [u8],
    fds: &mut PendingFds,
    max_fds: usize,
) -> io::Result<usize> {
    recv_with_policy(socket, bytes, fds, max_fds, cfg!(target_os = "macos"))
}

fn recv_with_policy(
    socket: &UnixStream,
    bytes: &mut [u8],
    fds: &mut PendingFds,
    max_fds: usize,
    explicit_cloexec: bool,
) -> io::Result<usize> {
    // Darwin has historically leaked truncated SCM_RIGHTS entries. Receive room
    // for XNU's 512-FD bound even at a zero application limit, then close extras
    // ourselves. Linux keeps kernel truncation and atomic close-on-exec.
    const WORDS: usize =
        (512 * mem::size_of::<libc::c_int>() + 64).div_ceil(mem::size_of::<usize>());
    let mut control = [0usize; WORDS];
    let remaining = max_fds.min(253).saturating_sub(fds.len());
    let capacity = if explicit_cloexec { 512 } else { remaining };
    let control_bytes = if capacity == 0 {
        0
    } else {
        // SAFETY: capacity is at most 512, so CMSG_SPACE cannot overflow.
        unsafe { libc::CMSG_SPACE((capacity * mem::size_of::<libc::c_int>()) as u32) as usize }
    };
    assert!(control_bytes <= mem::size_of_val(&control));
    let mut iov = libc::iovec {
        iov_base: bytes.as_mut_ptr().cast(),
        iov_len: bytes.len(),
    };
    // SAFETY: the kernel writes within live, aligned buffers. collect_fds takes
    // ownership of every returned descriptor, including those over our limit.
    unsafe {
        let mut msg: libc::msghdr = mem::zeroed();
        msg.msg_iov = &mut iov;
        msg.msg_iovlen = 1;
        if control_bytes != 0 {
            msg.msg_control = control.as_mut_ptr().cast();
            msg.msg_controllen = control_bytes as _;
        }
        let count = libc::recvmsg(
            socket.as_raw_fd(),
            &mut msg,
            if explicit_cloexec { 0 } else { RECV_FLAGS },
        );
        if count < 0 {
            return Err(io::Error::last_os_error());
        }
        collect_fds(&msg, fds, max_fds.min(253), |fd| {
            if explicit_cloexec {
                set_cloexec(fd)
            } else {
                Ok(())
            }
        })?;
        Ok(count as usize)
    }
}

#[cfg(test)]
#[path = "ancillary_tests.rs"]
mod tests;
