//! Linux TCP readiness for an exact, incomplete message body. SO_RCVLOWAT
//! preserves receive autotuning, unlike fixing SO_RCVBUF. The kernel clamps it
//! to available receive capacity and signals readiness when it is lowered.
use std::{
    io,
    os::fd::{AsFd, AsRawFd},
};

pub(super) fn get(socket: &impl AsFd) -> io::Result<usize> {
    let mut value: libc::c_int = 0;
    let mut length = std::mem::size_of_val(&value) as libc::socklen_t;
    // SAFETY: The borrowed descriptor remains live through the call. For
    // SOL_SOCKET/SO_RCVLOWAT, Linux writes one c_int and its socklen_t length;
    // both initialized stack objects are writable and live for the whole call.
    let result = unsafe {
        libc::getsockopt(
            socket.as_fd().as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_RCVLOWAT,
            std::ptr::from_mut(&mut value).cast(),
            &mut length,
        )
    };
    if result == -1 {
        return Err(io::Error::last_os_error());
    }
    if length as usize != std::mem::size_of_val(&value) || value < 1 {
        return Err(io::Error::other("invalid TCP receive low-water value"));
    }
    Ok(value as usize)
}

pub(super) fn set(socket: &impl AsFd, bytes: usize) -> io::Result<()> {
    let value = libc::c_int::try_from(bytes)
        .ok()
        .filter(|v| *v > 0)
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "invalid TCP receive minimum")
        })?;
    // SAFETY: The borrowed descriptor remains live. SO_RCVLOWAT consumes one
    // initialized c_int of exactly this size; setsockopt copies the value and
    // retains no pointer to this stack object.
    let result = unsafe {
        libc::setsockopt(
            socket.as_fd().as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_RCVLOWAT,
            std::ptr::from_ref(&value).cast(),
            std::mem::size_of_val(&value) as libc::socklen_t,
        )
    };
    if result == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
