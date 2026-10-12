//! Link and execute the same attachment-aware publication without an allocator.
use capntproto_rpc_engine::{exports, ids::WireId, io, messages};
use capntproto_rpc_wire::{
    attachments::Sender,
    publication,
    rpc_capnp::api::{CapDescriptor, Message, Payload, Return},
};
use core::{
    num::NonZeroUsize,
    task::{Context, Poll, Waker},
};

pub fn bounded_resource_output() {
    struct Sink {
        attachments: usize,
        bytes: usize,
    }
    impl capntproto_rpc_engine::io::Flush for Sink {
        type Error = ();
        fn poll_flush(
            &mut self,
            _: &mut core::task::Context<'_>,
        ) -> core::task::Poll<Result<(), capntproto_rpc_engine::io::IoError<Self::Error>>> {
            core::task::Poll::Ready(Ok(()))
        }
    }
    impl io::WriteResources<Sender<u16, 1>> for Sink {
        fn poll_write_resources(
            &mut self,
            _: &mut Context<'_>,
            bytes: &[u8],
            resources: Option<&Sender<u16, 1>>,
        ) -> Poll<Result<usize, io::IoError<()>>> {
            if let Some(resources) = resources {
                assert_eq!(self.bytes, 0);
                assert!(resources.iter().copied().eq([73]));
                self.attachments += resources.len();
            }
            assert!(!bytes.is_empty());
            self.bytes += 1;
            Poll::Ready(Ok(1))
        }
    }
    impl io::Close for Sink {
        fn poll_close(&mut self, _: &mut Context<'_>) -> Poll<Result<(), io::IoError<()>>> {
            Poll::Ready(Ok(()))
        }
    }
    let mut storage = [capnp::word(0, 0, 0, 0, 0, 0, 0, 0); 64];
    let mut buffers = [messages::Buffer::new(capnp::Word::words_to_bytes_mut(
        &mut storage,
    ))];
    let mut slots = [exports::Slot::EMPTY];
    exports::with_exports(&mut slots, |exports| {
        messages::with_messages(&mut buffers, &mut [messages::SendSlot::EMPTY], |messages| {
            let mut entries = [None];
            let mut queue = messages::Queue::with_resources(&mut entries).unwrap();
            let mut resources = Sender::<_, 1>::default();
            let index = resources.push(Some(73u16));
            let reserved = queue.reserve_with(resources).unwrap();
            let mut batch = exports.batch();
            let grant = batch.export(41u32).unwrap();
            let mut words = [capnp::word(0, 0, 0, 0, 0, 0, 0, 0); 32];
            let mut builder = capnp::message::FixedBuilder::new(&mut words).unwrap();
            let mut root = builder.try_init_root::<Message>().unwrap();
            let mut result = root.field(Message::RETURN).init().unwrap();
            result.scalar(Return::ANSWER_ID).set(7);
            let mut payload = result.field(Return::RESULTS).init().unwrap();
            payload
                .field(Payload::CONTENT)
                .set_capability_index(0)
                .unwrap();
            let mut descriptors = payload.field(Payload::CAP_TABLE).init(1).unwrap();
            let mut descriptor = descriptors.get_mut(0).unwrap();
            descriptor
                .scalar(CapDescriptor::SENDER_HOSTED)
                .set(grant.id().to_wire());
            descriptor.scalar(CapDescriptor::ATTACHED_FD).set(index);
            let writer = messages.reserve(512).unwrap();
            let length = {
                let mut bytes = messages.write(&writer);
                let length = bytes.len();
                let mut cursor = &mut *bytes;
                capnp::serialize::write_message_segments(
                    &mut cursor,
                    &builder.get_segments_for_output(),
                )
                .unwrap();
                length - cursor.len()
            };
            let lease = messages.seal(writer, length).unwrap();
            publication::prepare(batch, messages, lease, &mut [0], Default::default())
                .unwrap()
                .accept_into(reserved);
            let mut sink = Sink {
                attachments: 0,
                bytes: 0,
            };
            let mut cx = Context::from_waker(Waker::noop());
            let mut output = io::Output::new();
            for _ in 0..length {
                match output.poll(
                    &mut sink,
                    &mut queue,
                    messages,
                    &mut cx,
                    NonZeroUsize::new(1).unwrap(),
                ) {
                    Poll::Ready(Ok(())) => break,
                    Poll::Pending => (),
                    Poll::Ready(Err(_)) => panic!("output failed"),
                }
            }
            assert!(queue.is_empty());
            assert_eq!(
                io::poll_close(&mut sink, &mut cx, NonZeroUsize::new(1).unwrap()),
                Poll::Ready(Ok(()))
            );
            assert_eq!(sink.attachments, 1);
            assert_eq!(sink.bytes, length);
            assert_eq!(exports.release(grant.id(), 1).unwrap(), Some(41));
        })
        .unwrap();
    })
    .unwrap();
}
