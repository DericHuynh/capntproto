use super::*;

/// A generated request reader survives suspension and a nested outgoing call.
/// All storage, polling and wake policy come from the application.
// This fixture deliberately retains an immutable message lease across await;
// independent buffers must remain writable while that request is suspended.
#[allow(clippy::await_holding_refcell_ref)]
pub fn suspended_request_and_nested_call() -> Result<()> {
    use capntproto_rpc_engine::{
        messages::{self, Buffer, SendSlot},
        questions,
    };
    use core::{
        future::Future,
        task::{Context, Poll, Waker},
    };
    let mut source = [ZERO; 16];
    let mut builder = message::FixedBuilder::new(&mut source)?;
    let mut root = builder.try_init_root::<portable_capnp::api::BoundedRecord>()?;
    root.scalar(portable_capnp::api::BoundedRecord::ID).set(73);
    root.field(portable_capnp::api::BoundedRecord::TITLE)
        .copy_from("nested")?;
    let mut request = [ZERO; 17];
    let mut output = &mut Word::words_to_bytes_mut(&mut request)[..];
    capnp::serialize::write_message_segments(&mut output, &builder.get_segments_for_output())?;
    let used = 17 * 8 - output.len();
    let mut incoming = [ZERO; 17];
    let mut outgoing = [ZERO; 17];
    let mut buffers = [
        Buffer::new(Word::words_to_bytes_mut(&mut incoming)),
        Buffer::new(Word::words_to_bytes_mut(&mut outgoing)),
    ];
    let mut sends = [SendSlot::EMPTY];
    let mut slots = [questions::Slot::EMPTY];
    let future =
        messages::with_messages_async(&mut buffers, &mut sends, async |messages| -> Result<()> {
            let writer = messages.reserve(used).unwrap();
            messages.write(&writer)[..used]
                .copy_from_slice(&Word::words_to_bytes(&request)[..used]);
            let lease = messages.seal(writer, used).unwrap();
            let view = messages.read(&lease);
            let mut bytes = &*view;
            let decoded = capnp::serialize::read_message_from_flat_slice_no_alloc(
                &mut bytes,
                Default::default(),
            )?;
            let params = decoded
                .get_root::<portable_capnp::bounded_record::Reader<'_>>()?
                .into_api();
            questions::with_questions_async(&mut slots, async |questions| {
                let call = questions.reserve().unwrap().accept();
                let writer = messages.reserve(used).unwrap();
                messages.write(&writer)[..used].copy_from_slice(&view);
                let nested = messages.seal(writer, used).unwrap();
                let offer = messages.offer(&nested).unwrap();
                let send = messages.accept(offer);
                messages.release(nested);
                for position in 0..used {
                    let window = messages.send_window(send).unwrap();
                    assert_eq!(window.bytes()[0], view[position]);
                    assert_eq!(window.advance(1), Ok(position + 1 == used));
                    let mut resumed = false;
                    core::future::poll_fn(|cx| {
                        if resumed {
                            Poll::Ready(())
                        } else {
                            resumed = true;
                            cx.waker().wake_by_ref();
                            Poll::Pending
                        }
                    })
                    .await;
                    assert_eq!(params.id(), 73);
                    assert_eq!(params.title()?, "nested");
                }
                questions.returned(call.key().id(), false).unwrap();
                questions.release(call);
                questions.pending_finish().unwrap().accept();
                assert!(questions.is_empty());
                Ok::<_, capnp::Error>(())
            })
            .await
            .unwrap()?;

            drop(view);
            messages.release(lease);
            Ok(())
        });
    let mut pinned = core::pin::pin!(future);
    let mut cx = Context::from_waker(Waker::noop());
    for _ in 0..17 * 8 + 2 {
        if let Poll::Ready(value) = pinned.as_mut().poll(&mut cx) {
            return value.unwrap();
        }
    }
    panic!("bounded future exceeded poll budget");
}

/// Framed generated data survives partial sends and a retained local reader.
/// This is serialization/IO composition, not generated bounded RPC dispatch.
pub fn leased_message_roundtrip() -> Result<()> {
    use capntproto_rpc_engine::messages::{with_messages, Buffer, Error, SendSlot};
    use portable_capnp::api::BoundedRecord;
    let mut source = [ZERO; 32];
    let mut builder = message::FixedBuilder::new(&mut source)?;
    let mut root = builder.try_init_root::<BoundedRecord>()?;
    root.scalar(BoundedRecord::ID).set(73);
    root.field(BoundedRecord::TITLE)
        .copy_from("retained message")?;
    let mut wire = [ZERO; 33];
    let mut buffers = [Buffer::new(Word::words_to_bytes_mut(&mut wire))];
    let mut sends = [SendSlot::EMPTY];
    with_messages(&mut buffers, &mut sends, |messages| -> Result<()> {
        let writer = messages.reserve(33 * 8).unwrap();
        let capacity = messages.write(&writer).len();
        let mut view = messages.write(&writer);
        let mut output = &mut *view;
        capnp::serialize::write_message_segments(&mut output, &builder.get_segments_for_output())?;
        let used = capacity - output.len();
        drop(view);
        let lease = messages.seal(writer, used).unwrap();
        let retained = messages.retain(&lease).unwrap();
        let offer = messages.offer(&lease).unwrap();
        let key = messages.accept(offer);
        messages.release(lease);

        let mut destination = [ZERO; 33];
        let mut receiver = capnp::serialize::FrameReceiver::new(
            Word::words_to_bytes_mut(&mut destination),
            Default::default(),
        )?;
        for position in 0..used {
            {
                let pending = messages.send_window(key).unwrap();
                assert_eq!(pending.bytes().len(), used - position);
            }
            let send = messages.send_window(key).unwrap();
            let mut receive = receiver.input()?.unwrap();
            receive.bytes()[0] = send.bytes()[0];
            receive.commit(1)?;
            assert_eq!(send.advance(1), Ok(position + 1 == used));
        }
        assert!(receiver.input()?.is_none());
        let decoded = receiver.message()?.unwrap();
        let decoded = decoded
            .get_root::<portable_capnp::bounded_record::Reader<'_>>()?
            .into_api();
        assert_eq!(decoded.id(), 73);
        assert_eq!(decoded.title()?, "retained message");
        assert_eq!(messages.reserve(1).unwrap_err(), Error::NoBuffer);
        {
            let view = messages.read(&retained);
            let mut input = &*view;
            let view = capnp::serialize::read_message_from_flat_slice_no_alloc(
                &mut input,
                Default::default(),
            )?;
            assert!(input.is_empty());
            assert_eq!(
                view.get_root::<portable_capnp::bounded_record::Reader<'_>>()?
                    .into_api()
                    .title()?,
                "retained message"
            );
        }
        messages.release(retained);
        let writer = messages.reserve(33 * 8).unwrap();
        assert!(messages.write(&writer).iter().all(|&byte| byte == 0));
        messages.write(&writer)[0] = 19;
        assert_eq!(messages.fail_send(key), Err(Error::StaleSend));
        assert_eq!(messages.write(&writer)[0], 19);
        messages.discard(writer);
        Ok(())
    })
    .unwrap()
}

/// A mutable generated result is built directly in its reserved wire buffer
/// across suspension while another buffer completes output and is reused.
#[allow(clippy::await_holding_refcell_ref)]
pub fn suspended_result_and_nested_output() -> Result<()> {
    use capntproto_rpc_engine::messages;
    use core::{
        future::Future,
        task::{Context, Poll, Waker},
    };
    let mut a = [ZERO; 33];
    let mut b = [ZERO; 33];
    let mut buffers = [
        messages::Buffer::new(Word::words_to_bytes_mut(&mut a)),
        messages::Buffer::new(Word::words_to_bytes_mut(&mut b)),
    ];
    let mut sends = [messages::SendSlot::EMPTY];
    let future =
        messages::with_messages_async(&mut buffers, &mut sends, async |messages| -> Result<()> {
            let mut writer = messages.reserve(33 * 8).unwrap();
            let mut view = messages.write_borrowed(&mut writer).unwrap();
            let words;
            {
                let mut builder = message::FixedBuilder::from_bytes(&mut view[8..])?;
                let mut root = builder.try_init_root::<portable_capnp::api::BoundedRecord>()?;
                root.scalar(portable_capnp::api::BoundedRecord::ID).set(73);
                let mut suspended = false;
                core::future::poll_fn(|cx| {
                    if suspended {
                        return Poll::Ready(());
                    }
                    let other = messages.reserve(4).unwrap();
                    messages.write(&other)[..4].copy_from_slice(b"side");
                    let lease = messages.seal(other, 4).unwrap();
                    let offer = messages.offer(&lease).unwrap();
                    let send = messages.accept(offer);
                    messages.release(lease);
                    assert!(!messages.send_window(send).unwrap().advance(1).unwrap());
                    assert!(messages.send_window(send).unwrap().advance(3).unwrap());
                    let other = messages.reserve(4).unwrap();
                    messages.write(&other)[..4].copy_from_slice(b"next");
                    messages.discard(other);
                    suspended = true;
                    cx.waker().wake_by_ref();
                    Poll::Pending
                })
                .await;
                assert_eq!(root.read().id(), 73);
                root.field(portable_capnp::api::BoundedRecord::TITLE)
                    .copy_from("after suspension")?;
                words = builder.into_segment().len() / 8;
            }
            view[..4].copy_from_slice(&0u32.to_le_bytes());
            view[4..8].copy_from_slice(&(words as u32).to_le_bytes());
            drop(view);
            let lease = messages.seal(writer, words * 8 + 8).unwrap();
            let view = messages.read(&lease);
            let mut bytes = &*view;
            let reader = capnp::serialize::read_message_from_flat_slice_no_alloc(
                &mut bytes,
                Default::default(),
            )?;
            let result = reader
                .get_root::<portable_capnp::bounded_record::Reader<'_>>()?
                .into_api();
            assert_eq!(result.id(), 73);
            assert_eq!(result.title()?, "after suspension");
            drop(view);
            messages.release(lease);
            Ok(())
        });
    let mut future = core::pin::pin!(future);
    let mut cx = Context::from_waker(Waker::noop());
    assert!(future.as_mut().poll(&mut cx).is_pending());
    let Poll::Ready(result) = future.as_mut().poll(&mut cx) else {
        panic!("fixture poll budget");
    };
    result.unwrap()
}
