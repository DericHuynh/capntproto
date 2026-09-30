// Copyright (c) 2026 ReProto contributors. Licensed under the MIT license.
//! Descriptor ownership at the message boundary. Non-Unix transports use an
//! empty attachment list; dropping attachments must not break RPC capabilities.
use capnp::private::capability::ClientHook;

#[cfg(unix)]
pub(crate) type AttachedFd = std::rc::Rc<std::os::fd::OwnedFd>;
#[cfg(not(unix))]
pub(crate) type AttachedFd = std::convert::Infallible;

#[derive(Default)]
pub(crate) struct OutgoingFds {
    #[cfg(unix)]
    fds: Vec<AttachedFd>,
}
impl OutgoingFds {
    pub(crate) fn add(
        &mut self,
        _cap: &dyn ClientHook,
        mut _descriptor: crate::rpc_capnp::cap_descriptor::Builder<'_>,
    ) {
        #[cfg(unix)]
        if self.fds.len() < 253 && _descriptor.reborrow().get_attached_fd() == 255 {
            if let Some(fd) = _cap.get_fd() {
                _descriptor.set_attached_fd(self.fds.len() as u8);
                self.fds.push(fd);
            }
        }
    }
    pub(crate) fn attach(self, _message: &mut dyn crate::OutgoingMessage) {
        #[cfg(unix)]
        _message.set_fds(self.fds);
    }
}

pub(crate) struct IncomingFds {
    #[cfg(unix)]
    fds: Vec<Option<AttachedFd>>,
}
impl IncomingFds {
    pub(crate) fn new(_message: &mut dyn crate::IncomingMessage) -> Self {
        Self {
            #[cfg(unix)]
            fds: _message
                .take_fds()
                .into_iter()
                .take(253)
                .map(|fd| Some(std::rc::Rc::new(fd)))
                .collect(),
        }
    }
    pub(crate) fn take(&mut self, _index: u8) -> Option<AttachedFd> {
        #[cfg(unix)]
        {
            self.fds.get_mut(_index as usize).and_then(Option::take)
        }
        #[cfg(not(unix))]
        {
            None
        }
    }
}
