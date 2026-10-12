//! Caller-polled framed IO for the portable engine's resource-owning FIFO.
#![forbid(unsafe_code)]
use super::*;
use capnp_rpc::bounded::attachments::ImportReceiver;
use capnp_rpc::bounded::attachments::Sender;
use capnp_rpc::scoped::wire::{coordinator::transport::Input, resources::Import};
use capntproto_rpc_engine::ids::ImportId;
use capntproto_rpc_engine::io::{IoError, WriteResources};
use std::task::{Context, Poll};

/// Exact-frame reader for the scoped coordinator. The frame decoder supplies
/// bounded slices, so recvmsg cannot prefetch another message's descriptors.
/// Exclusive read ownership and peer authorization belong to the caller.
pub struct ResourceReader<'a, F> {
    socket: &'a UnixStream,
    pending: PendingFds,
    maximum: usize,
    make: F,
}
impl<'a, F> ResourceReader<'a, F> {
    pub fn new(socket: &'a UnixStream, maximum: usize, make: F) -> Self {
        Self {
            socket,
            pending: PendingFds::default(),
            maximum: maximum.min(253),
            make,
        }
    }
}
impl<F> capntproto_rpc_engine::io::Read for ResourceReader<'_, F> {
    type Error = io::Error;
    fn poll_read(
        &mut self,
        cx: &mut Context<'_>,
        bytes: &mut [u8],
    ) -> Poll<Result<usize, IoError<Self::Error>>> {
        for _ in 0..8 {
            match self.socket.poll_read_ready(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Err(error)) => return Poll::Ready(Err(IoError::Failed(error))),
                Poll::Ready(Ok(())) => (),
            }
            match self.socket.try_io(Interest::READABLE, || {
                recv_once(self.socket, bytes, &mut self.pending, self.maximum)
            }) {
                Ok(count) => return Poll::Ready(Ok(count)),
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => (),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {
                    return Poll::Ready(Err(IoError::Interrupted))
                }
                Err(error) => return Poll::Ready(Err(IoError::Failed(error))),
            }
        }
        cx.waker().wake_by_ref();
        Poll::Pending
    }
}
impl<T, F: FnMut(ImportId) -> T> ImportReceiver<Import<T>> for ResourceReader<'_, F> {
    fn make_import(&mut self, id: ImportId) -> Import<T> {
        Import::new((self.make)(id))
    }
    fn attach(&mut self, owner: Option<&mut Import<T>>, index: u8) {
        // Taking even null/reflected entries prevents duplicate indices from
        // transferring the same kernel descriptor to a different capability.
        if let Some(fd) = self
            .pending
            .slots
            .get_mut(usize::from(index))
            .and_then(Option::take)
        {
            if let Some(owner) = owner {
                owner.attach(Rc::new(fd));
            }
        }
    }
}
impl<T, F: FnMut(ImportId) -> T> Input<Import<T>> for ResourceReader<'_, F> {
    fn finish_frame(&mut self) {
        self.pending.clear();
    }
}

/// Borrow a connected, authorized Unix stream for portable FIFO output. The
/// caller must provide exclusive write ownership for the lifetime of the queue:
/// unrelated writes would break message framing. No extra output buffer or
/// protocol ledger is created. Dropping a poll never loses queued resources.
pub struct ResourceWriter<'a> {
    socket: &'a UnixStream,
}
impl<'a> ResourceWriter<'a> {
    pub fn new(socket: &'a UnixStream) -> Self {
        Self { socket }
    }
}
impl capntproto_rpc_engine::io::Flush for ResourceWriter<'_> {
    type Error = io::Error;
    fn poll_flush(&mut self, _: &mut Context<'_>) -> Poll<Result<(), IoError<Self::Error>>> {
        Poll::Ready(Ok(()))
    }
}
impl capntproto_rpc_engine::io::Close for ResourceWriter<'_> {
    fn poll_close(&mut self, _: &mut Context<'_>) -> Poll<Result<(), IoError<Self::Error>>> {
        Poll::Ready(
            socket2::SockRef::from(self.socket)
                .shutdown(std::net::Shutdown::Write)
                .map_err(|error| {
                    if error.kind() == io::ErrorKind::Interrupted {
                        IoError::Interrupted
                    } else {
                        IoError::Failed(error)
                    }
                }),
        )
    }
}
impl<const N: usize> WriteResources<Sender<Rc<OwnedFd>, N>> for ResourceWriter<'_> {
    fn poll_write_resources(
        &mut self,
        cx: &mut Context<'_>,
        bytes: &[u8],
        resources: Option<&Sender<Rc<OwnedFd>, N>>,
    ) -> Poll<Result<usize, IoError<Self::Error>>> {
        // Recheck readiness after WouldBlock; Tokio clears the stale writable
        // indication inside try_io. Bound spurious readiness too.
        for _ in 0..8 {
            match self.socket.poll_write_ready(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Err(error)) => return Poll::Ready(Err(IoError::Failed(error))),
                Poll::Ready(Ok(())) => (),
            }
            match self.socket.try_io(Interest::WRITABLE, || match resources {
                Some(resources) => ancillary::send_resources(self.socket, bytes, resources.iter()),
                None => send_once(self.socket, bytes, &[]),
            }) {
                Ok(count) => return Poll::Ready(Ok(count)),
                Err(error) if error.kind() == io::ErrorKind::WouldBlock => (),
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {
                    return Poll::Ready(Err(IoError::Interrupted))
                }
                Err(error) => return Poll::Ready(Err(IoError::Failed(error))),
            }
        }
        cx.waker().wake_by_ref();
        Poll::Pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use capntproto_rpc_engine::{io::poll_queue_resources, messages};
    use std::{num::NonZeroUsize, task::Poll};

    #[tokio::test(flavor = "current_thread")]
    async fn portable_close_signals_eof_without_closing_the_read_half() {
        let (socket, peer) = UnixStream::pair().unwrap();
        let mut writer = ResourceWriter::new(&socket);
        futures::future::poll_fn(|cx| {
            capntproto_rpc_engine::io::Flush::poll_flush(&mut writer, cx)
        })
        .await
        .unwrap();
        futures::future::poll_fn(|cx| {
            capntproto_rpc_engine::io::poll_close(&mut writer, cx, NonZeroUsize::new(2).unwrap())
        })
        .await
        .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(2), peer.readable())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            peer.try_read(&mut [0]).unwrap(),
            0,
            "peer must observe write-half closure"
        );
        peer.writable().await.unwrap();
        assert_eq!(peer.try_write(b"x").unwrap(), 1);
        tokio::time::timeout(std::time::Duration::from_secs(2), socket.readable())
            .await
            .unwrap()
            .unwrap();
        let mut byte = [0];
        assert_eq!(socket.try_read(&mut byte).unwrap(), 1);
        assert_eq!(&byte, b"x");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn resource_reader_isolates_frames_and_drops_duplicate_and_unused_descriptors() {
        use capntproto_rpc_engine::io::Read;
        let (sender, receiver) = UnixStream::pair().unwrap();
        let mut writer = ResourceWriter::new(&sender);
        let mut reader = ResourceReader::new(&receiver, 2, |_| ());
        let mut peers = Vec::new();
        for bytes in [b"ab", b"cd"] {
            let mut bundle = Sender::<_, 2>::default();
            for _ in 0..2 {
                let (owned, peer) = std::os::unix::net::UnixStream::pair().unwrap();
                peer.set_read_timeout(Some(std::time::Duration::from_secs(2)))
                    .unwrap();
                peers.push(peer);
                bundle.push(Some(Rc::new(OwnedFd::from(owned))));
            }
            assert_eq!(
                futures::future::poll_fn(|cx| writer.poll_write_resources(
                    cx,
                    bytes,
                    Some(&bundle)
                ))
                .await
                .unwrap(),
                2
            );
        }
        let mut owner = Import::new(());
        for expected in [b"ab", b"cd"] {
            let mut bytes = [0; 2];
            for byte in &mut bytes {
                assert_eq!(
                    futures::future::poll_fn(|cx| reader.poll_read(cx, std::slice::from_mut(byte)))
                        .await
                        .unwrap(),
                    1
                );
            }
            assert_eq!(&bytes, expected);
            assert_eq!(reader.pending.len(), 2);
            // A null descriptor consumes index zero. It cannot later be
            // reassigned to another import by a duplicate descriptor index.
            reader.attach(None, 0);
            reader.attach(Some(&mut owner), 0);
            assert!(owner.metadata().fd.is_none());
            reader.finish_frame();
            assert!(reader.pending.is_empty());
        }
        for mut peer in peers {
            assert_eq!(std::io::Read::read(&mut peer, &mut [0]).unwrap(), 0);
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn portable_fifo_transfers_live_fd_once_across_forced_partial_writes() {
        let (socket, receiver) = UnixStream::pair().unwrap();
        let (resource, mut resource_peer) = std::os::unix::net::UnixStream::pair().unwrap();
        resource_peer
            .set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .unwrap();
        let fd = Rc::new(OwnedFd::from(resource));
        let weak = Rc::downgrade(&fd);
        let mut attachments = Sender::<_, 1>::default();
        assert_eq!(attachments.push(Some(fd)), 0);
        let mut received = PendingFds::default();
        let mut bytes = [0; 3];
        let mut buffers = [messages::Buffer::new(&mut bytes)];
        messages::with_messages_async(
            &mut buffers,
            &mut [messages::SendSlot::EMPTY],
            async |messages| {
                let mut entries = [None];
                let mut queue = messages::Queue::with_resources(&mut entries).unwrap();
                let writer = messages.reserve(3).unwrap();
                messages.write(&writer).copy_from_slice(b"abc");
                let lease = messages.seal(writer, 3).unwrap();
                let offer = messages.offer(&lease).unwrap();
                queue
                    .reserve_with(attachments)
                    .unwrap()
                    .accept(messages, offer);
                messages.release(lease);
                struct Partial<'a>(ResourceWriter<'a>);
                impl capntproto_rpc_engine::io::Flush for Partial<'_> {
                    type Error = io::Error;
                    fn poll_flush(
                        &mut self,
                        cx: &mut Context<'_>,
                    ) -> Poll<Result<(), IoError<Self::Error>>> {
                        self.0.poll_flush(cx)
                    }
                }
                impl WriteResources<Sender<Rc<OwnedFd>, 1>> for Partial<'_> {
                    fn poll_write_resources(
                        &mut self,
                        cx: &mut Context<'_>,
                        bytes: &[u8],
                        resources: Option<&Sender<Rc<OwnedFd>, 1>>,
                    ) -> Poll<Result<usize, IoError<Self::Error>>> {
                        self.0.poll_write_resources(cx, &bytes[..1], resources)
                    }
                }
                let mut writer = Partial(ResourceWriter::new(&socket));
                let mut output = [0; 3];
                for byte in &mut output {
                    // One successful byte per poll; Pending does not lose the queued
                    // bundle. The final send drops our descriptor after kernel IO.
                    futures::future::poll_fn(|cx| {
                        let before = queue.window(messages).unwrap().unwrap().bytes().len();
                        let result = poll_queue_resources(
                            &mut writer,
                            &mut queue,
                            messages,
                            cx,
                            NonZeroUsize::new(1).unwrap(),
                        );
                        if let Poll::Ready(Err(error)) = result {
                            panic!("{error:?}");
                        }
                        if queue.is_empty()
                            || queue.window(messages).unwrap().unwrap().bytes().len() < before
                        {
                            Poll::Ready(())
                        } else {
                            Poll::Pending
                        }
                    })
                    .await;
                    loop {
                        receiver.readable().await.unwrap();
                        match receiver.try_io(Interest::READABLE, || {
                            recv_once(&receiver, std::slice::from_mut(byte), &mut received, 253)
                        }) {
                            Ok(1) => break,
                            Err(error) if error.kind() == io::ErrorKind::WouldBlock => (),
                            other => panic!("{other:?}"),
                        }
                    }
                    assert_eq!(received.len(), 1);
                }
                assert_eq!(&output, b"abc");
                assert!(queue.is_empty());
            },
        )
        .await
        .unwrap();
        assert!(weak.upgrade().is_none());
        let received_fd = received.drain().next().unwrap();
        let mut received_stream = std::os::unix::net::UnixStream::from(received_fd);
        std::io::Write::write_all(&mut received_stream, b"x").unwrap();
        let mut response = [0];
        std::io::Read::read_exact(&mut resource_peer, &mut response).unwrap();
        assert_eq!(&response, b"x");
        drop(received_stream);
        assert_eq!(
            std::io::Read::read(&mut resource_peer, &mut response).unwrap(),
            0
        );
    }
}
