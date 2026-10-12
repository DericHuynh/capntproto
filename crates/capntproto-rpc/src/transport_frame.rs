//! Immutable wire messages at the hosted transport boundary.
use capnp::{message::ReaderSegments, Error};

/// An owned, structurally framed message for a message-oriented transport.
/// Construction retains or copies segment bytes verbatim: capability indices, descriptor
/// slots, far pointers and unreachable words are never traversed or rewritten.
/// It validates framing, not RPC semantics or capability authority.
///
/// Transport queues must retain this independent byte owner through writing.
pub struct OutgoingFrame {
    pub(crate) credit: Option<std::rc::Rc<crate::admission::Permit>>,
    body: Body,
    #[cfg(unix)]
    fds: Vec<std::rc::Rc<std::os::fd::OwnedFd>>,
}
enum Body {
    Copied(capnp::serialize::OwnedSegments),
    Shared(capnp::serialize::NoAllocBufferSegments<capntproto_rpc_engine::messages::SharedBytes>),
}
impl Body {
    fn segments(&self) -> &dyn ReaderSegments {
        match self {
            Self::Copied(segments) => segments,
            Self::Shared(segments) => segments,
        }
    }
}
impl OutgoingFrame {
    /// Copy exactly one standard (unpacked) stream frame. Validate its complete
    /// extent before allocating, so a truncated size header cannot cause a large
    /// allocation. This operation does not extract capability hooks.
    pub fn copy_from(bytes: &[u8]) -> capnp::Result<Self> {
        let options = capnp::message::ReaderOptions {
            traversal_limit_in_words: None,
            ..Default::default()
        };
        Self::validate(bytes)?;
        Ok(Self {
            credit: None,
            body: Body::Copied(capnp::serialize::read_message(bytes, options)?.into_segments()),
            #[cfg(unix)]
            fds: Vec::new(),
        })
    }
    pub(crate) fn from_shared(
        bytes: capntproto_rpc_engine::messages::SharedBytes,
    ) -> capnp::Result<Self> {
        let options = capnp::message::ReaderOptions {
            traversal_limit_in_words: None,
            ..Default::default()
        };
        Ok(Self {
            credit: None,
            body: Body::Shared(capnp::serialize::NoAllocBufferSegments::from_buffer_exact(
                bytes, options,
            )?),
            #[cfg(unix)]
            fds: Vec::new(),
        })
    }
    fn validate(bytes: &[u8]) -> capnp::Result<()> {
        // Framing is byte-oriented. A borrowed no-alloc reader would reject
        // valid unaligned IO slices, notably Vec<u8> on 32-bit targets. Validate
        // lengths without dereferencing words; the owned copy below is aligned.
        let header = capnp::serialize::SegmentTableHeader::parse(bytes)?;
        let table = bytes
            .get(..header.table_bytes())
            .ok_or_else(|| Error::from_kind(capnp::ErrorKind::PrematureEndOfFile))?;
        let mut length = table.len();
        for index in 0..header.segment_count() {
            let at = (index + 1) * 4;
            let words = u32::from_le_bytes(table[at..at + 4].try_into().unwrap()) as usize;
            length = words
                .checked_mul(8)
                .and_then(|n| length.checked_add(n))
                .ok_or_else(|| Error::from_kind(capnp::ErrorKind::MessageSizeOverflow))?;
        }
        if length > bytes.len() {
            return Err(Error::from_kind(capnp::ErrorKind::PrematureEndOfFile));
        }
        if length < bytes.len() {
            return Err(Error::failed(
                "outgoing frame contains trailing bytes".into(),
            ));
        }
        Ok(())
    }
    pub fn segments(&self) -> capnp::OutputSegments<'_> {
        let segments = self.body.segments();
        if segments.len() == 1 {
            capnp::OutputSegments::SingleSegment([segments.get_segment(0).unwrap()])
        } else {
            capnp::OutputSegments::MultiSegment(
                (0..segments.len())
                    .map(|i| segments.get_segment(i as u32).unwrap())
                    .collect(),
            )
        }
    }
    pub fn size_in_words(&self) -> usize {
        if let Body::Shared(segments) = &self.body {
            return (segments.buffer().len() - (segments.len() / 2 + 1) * 8) / 8;
        }
        let segments = self.body.segments();
        (0..segments.len())
            .map(|id| segments.get_segment(id as u32).unwrap().len() / 8)
            .sum()
    }
    /// Already validated complete framing, retained through the write and flush.
    pub(crate) fn framed_bytes(&self) -> Option<&[u8]> {
        match &self.body {
            Body::Shared(segments) => Some(segments.buffer()),
            _ => None,
        }
    }
    /// Associate transport-owned descriptors with this exact frame. The wire's
    /// 255 sentinel is never a descriptor index; Unix transports may cap at 253.
    #[cfg(unix)]
    pub fn set_fds(&mut self, fds: Vec<std::rc::Rc<std::os::fd::OwnedFd>>) {
        self.fds = fds.into_iter().take(255).collect();
    }
    #[cfg(unix)]
    pub fn take_fds(&mut self) -> Vec<std::rc::Rc<std::os::fd::OwnedFd>> {
        std::mem::take(&mut self.fds)
    }
}
