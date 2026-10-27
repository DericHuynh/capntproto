//! Exclusive framed Unix IO; the existing writer owns ordered physical output.
use super::*;
use std::task::{Context, Poll, Waker};

#[derive(Default)]
pub(super) struct State {
    next: Cell<u64>,
    completed: Cell<u64>,
    observed: Cell<bool>,
    terminal: RefCell<Option<capnp::Result<()>>>,
    flush_wake: RefCell<Option<Waker>>,
    finish_wake: RefCell<Option<Waker>>,
}
impl State {
    pub(super) fn complete(&self, sequence: u64) {
        if sequence != 0 {
            self.completed.set(sequence);
            let wake = self.flush_wake.borrow_mut().take();
            if let Some(wake) = wake {
                wake.wake();
            }
        }
    }
    pub(super) fn stop(&self, result: capnp::Result<()>) {
        if self.terminal.borrow().is_some() {
            return;
        }
        *self.terminal.borrow_mut() = Some(result);
        let flush = self.flush_wake.borrow_mut().take();
        let finish = self.finish_wake.borrow_mut().take();
        if let Some(wake) = flush {
            wake.wake();
        }
        if let Some(wake) = finish {
            wake.wake();
        }
    }
    fn finished(self: &Rc<Self>) -> Promise<(), Error> {
        let state = self.clone();
        Promise::from_future(std::future::poll_fn(move |cx| {
            if let Some(result) = state.terminal.borrow().as_ref() {
                return Poll::Ready(result.clone());
            }
            let wake = cx.waker().clone();
            let old = state.finish_wake.replace(Some(wake));
            drop(old);
            Poll::Pending
        }))
    }
}

pub(super) fn take(inner: &Rc<Inner>) -> capnp::Result<capnp_rpc::FrameIo> {
    if inner.closed.get() {
        return Err(stopped());
    }
    if inner.reading.get() || inner.frame_output.observed.get() {
        return Err(Error::failed("Unix frame IO already owned".into()));
    }
    let input = inner.input.borrow_mut().take().ok_or_else(stopped)?;
    inner.reading.set(true);
    inner.frame_output.observed.set(true);
    Ok(capnp_rpc::FrameIo {
        incoming: Box::new(Input(ReadGuard {
            inner: inner.clone(),
            input: Some(input),
            complete: false,
        })),
        outgoing: Box::new(Output {
            inner: inner.clone(),
            through: inner.frame_output.next.get(),
            reservation: None,
        }),
        write_finished: Some(inner.frame_output.finished()),
    })
}

struct Input(ReadGuard);
impl Input {
    fn reader(&self) -> &FdReader<Rc<UnixStream>> {
        self.0.input.as_ref().expect("exclusive frame input")
    }
    fn reader_mut(&mut self) -> &mut FdReader<Rc<UnixStream>> {
        self.0.input.as_mut().expect("exclusive frame input")
    }
    fn descriptors(&mut self) -> Vec<OwnedFd> {
        let reader = self.reader();
        let mut ancillary = reader.reader.get_ref().ancillary.borrow_mut();
        if ancillary.end <= reader.reader.consumed_bytes() {
            ancillary.fds.drain().collect()
        } else {
            Vec::new()
        }
    }
}
impl capnp_rpc::IncomingFrameStream for Input {
    fn buffered_next_frame_size(&mut self) -> Option<core::num::NonZeroUsize> {
        self.reader_mut().reader.buffered_next_frame_size()
    }
    fn poll_frame(&mut self, cx: &mut Context<'_>) -> Poll<capnp::Result<Option<&[u8]>>> {
        self.reader_mut().reader.poll_borrowed_frame(cx)
    }
    fn frame(&self) -> Option<&[u8]> {
        self.reader().reader.borrowed_frame()
    }
    fn share_frame(&mut self) -> Option<(std::sync::Arc<Vec<u8>>, core::ops::Range<usize>)> {
        self.reader_mut().reader.share_borrowed_frame()
    }
    fn poll_frame_size(&mut self, cx: &mut Context<'_>) -> Poll<capnp::Result<Option<usize>>> {
        self.reader_mut().reader.poll_stream_frame_size(cx)
    }
    fn poll_read_frame(
        &mut self,
        cx: &mut Context<'_>,
        bytes: &mut [u8],
    ) -> Poll<capnp::Result<usize>> {
        self.reader_mut().reader.poll_stream_frame(cx, bytes)
    }
    fn take_fds(&mut self) -> Vec<OwnedFd> {
        self.descriptors()
    }
    fn finish_frame(&mut self) {
        drop(self.descriptors());
        self.reader_mut().reader.finish_borrowed_frame();
    }
}

struct Output {
    inner: Rc<Inner>,
    through: u64,
    reservation: Option<(u64, super::queue::Pending)>,
}
impl Drop for Output {
    fn drop(&mut self) {
        self.inner.frame_output.observed.set(false);
        let wake = self.inner.frame_output.flush_wake.borrow_mut().take();
        drop(wake);
    }
}
impl capnp_rpc::OutgoingMessageStream for Output {
    fn shutdown(&mut self, result: capnp::Result<()>) -> Promise<(), Error> {
        Endpoint(self.inner.clone()).shutdown(result)
    }
    fn reserve(&mut self) -> capnp::Result<()> {
        assert!(self.reservation.is_none(), "one Unix frame reservation");
        if self.inner.closed.get() {
            return Err(stopped());
        }
        if let Some(result) = self.inner.frame_output.terminal.borrow().as_ref() {
            return Err(result.clone().err().unwrap_or_else(stopped));
        }
        let sequence = self
            .inner
            .frame_output
            .next
            .get()
            .checked_add(1)
            .ok_or_else(|| Error::overloaded("Unix output sequence exhausted".into()))?;
        let pending = self.inner.metrics.enqueue(0)?;
        self.inner.frame_output.next.set(sequence);
        self.reservation = Some((sequence, pending));
        Ok(())
    }
    fn cancel_reservation(&mut self) {
        self.reservation = None;
    }
    fn publish_reserved(&mut self, mut frame: capnp_rpc::OutgoingFrame) -> capnp::Result<()> {
        let (sequence, pending) = self.reservation.take().expect("reserved Unix publication");
        pending.set_bytes(frame.size_in_words() * 8);
        let fds = if self.inner.send_fds {
            frame.take_fds().into_iter().take(253).collect()
        } else {
            drop(frame.take_fds());
            Vec::new()
        };
        self.inner
            .sender
            .unbounded_send(Write::Frame(frame, fds, None, pending, sequence))
            .map_err(|_| stopped())?;
        self.inner.frame_output.next.set(sequence);
        self.through = sequence;
        Ok(())
    }
    fn poll_flush(&mut self, cx: &mut Context<'_>) -> Poll<capnp::Result<()>> {
        let state = &self.inner.frame_output;
        if state.completed.get() >= self.through {
            return Poll::Ready(Ok(()));
        }
        if let Some(result) = state.terminal.borrow().as_ref() {
            return Poll::Ready(Err(result.clone().err().unwrap_or_else(stopped)));
        }
        let wake = cx.waker().clone();
        let old = state.flush_wake.replace(Some(wake));
        drop(old);
        Poll::Pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{future::Future, pin::Pin};

    #[tokio::test(flavor = "current_thread")]
    async fn exclusive_frames_keep_descriptors_at_their_admission_boundary() {
        let (left, right) = UnixStream::pair().unwrap();
        let mut sending = VatNetwork::new(left, Side::Client, Options::default());
        let mut receiving = VatNetwork::new(right, Side::Server, Options::default());
        let mut left = capnp_rpc::VatNetwork::connect(&mut sending, Side::Server).unwrap();
        let mut right = capnp_rpc::VatNetwork::connect(&mut receiving, Side::Client).unwrap();
        let mut output = left.take_frame_io().unwrap();
        let mut input = right.take_frame_io().unwrap();
        assert!(left.take_frame_io().is_err());
        assert!(right.take_frame_io().is_err());
        let mut driver = capnp_rpc::VatNetwork::take_output_driver(&mut sending).unwrap();
        let descriptor = super::super::tests::descriptor(37);
        let mut expected = Vec::new();
        for (index, text) in ["descriptor frame", "bare successor"]
            .into_iter()
            .enumerate()
        {
            let mut message = Builder::new_default();
            message.set_root(text).unwrap();
            let bytes = capnp::serialize::write_message_to_words(&message);
            let mut frame = capnp_rpc::OutgoingFrame::copy_from(&bytes).unwrap();
            if index == 0 {
                frame.set_fds(vec![descriptor.clone()]);
            }
            output.outgoing.send(frame).unwrap();
            expected.push(bytes);
        }
        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            for (index, bytes) in expected.iter().enumerate() {
                let received = std::future::poll_fn(|cx| {
                    if let Poll::Ready(result) = Pin::new(&mut driver).poll(cx) {
                        result.unwrap();
                    }
                    input
                        .incoming
                        .poll_frame(cx)
                        .map(|result| result.unwrap().unwrap().to_vec())
                })
                .await;
                assert_eq!(&received, bytes);
                let descriptors = input.incoming.take_fds();
                assert_eq!(descriptors.len(), usize::from(index == 0));
                assert!(input.incoming.take_fds().is_empty());
                // Re-polling and sharing the current frame cannot consume the
                // successor or deliver its descriptors before finish_frame.
                assert_eq!(input.incoming.frame().unwrap(), bytes);
                let retained = input.incoming.share_frame();
                input.incoming.finish_frame();
                if let Some((owner, range)) = retained {
                    assert_eq!(&owner[range], bytes);
                }
            }
            std::future::poll_fn(|cx| {
                if let Poll::Ready(result) = Pin::new(&mut driver).poll(cx) {
                    result.unwrap();
                }
                output.outgoing.poll_flush(cx)
            })
            .await
            .unwrap();
        })
        .await
        .unwrap();
    }
}
