//! Suspended output owns its buffer and every capability until publication.
use super::{
    driver::with_owned_peer,
    driver_invocation::close,
    portable_capnp::{api::echo::CapabilityResults, echo},
    ZERO,
};
use capnp::field_api::Struct;
use capntproto_rpc_engine::{clients, messages};
use capntproto_rpc_wire::{clients as binding, owned::connection::DraftError};
use core::{
    future::Future,
    task::{Context, Poll, Waker},
};

pub fn owned_result_draft(mode: u8) {
    let mut words = [[ZERO; 97]; 3];
    let mut buffers = words
        .each_mut()
        .map(|words| messages::Buffer::new(capnp::Word::words_to_bytes_mut(words)));
    super::driver::message_scope(&mut buffers, &mut [messages::SendSlot::EMPTY], |messages| {
        let mut sends = [None];
        let mut queue = super::driver::message_queue(&mut sends).unwrap();
        with_owned_peer(|mut peer| {
            let target = peer
                .runtime
                .endpoint(binding::Route::local(43), clients::Location::Local)
                .unwrap();
            let expected = target.key();
            assert!(peer
                .local_result_draft(echo::Owned::CAPABILITY, messages, 777, Default::default())
                .is_err());
            let mut draft = peer
                .local_result_draft(echo::Owned::CAPABILITY, messages, 776, Default::default())
                .unwrap();
            let mut cx = Context::from_waker(Waker::noop());
            {
                let future = draft.write_async(messages, async |content, capabilities| {
                    let mut results = content.cast::<Struct<CapabilityResults>>().init()?;
                    let owned = peer.runtime.retain(&target).unwrap();
                    capabilities
                        .set(results.field(CapabilityResults::VALUE), owned)
                        .unwrap_or_else(|_| panic!("first owner"));
                    assert_eq!(capabilities.len(), 1);
                    assert!(!capabilities.is_empty());
                    if mode == 4 {
                        for _ in 1..4 {
                            let owned = peer.runtime.retain(&target).unwrap();
                            capabilities
                                .set(results.field(CapabilityResults::VALUE), owned)
                                .unwrap_or_else(|_| panic!("retained owner"));
                        }
                        let owned = peer.runtime.retain(&target).unwrap();
                        let rejected = capabilities
                            .set(results.field(CapabilityResults::VALUE), owned)
                            .err()
                            .unwrap();
                        assert_eq!(rejected.error, capntproto_rpc_wire::owned::Error::Capacity);
                        assert_eq!(rejected.client.key(), expected);
                        peer.runtime.release(rejected.client);
                        assert_eq!(capabilities.len(), 4);
                    }
                    if mode == 3 {
                        // Deliberate use of the low-level wire escape hatch must
                        // still fail final validation without leaking owners.
                        results.field(CapabilityResults::VALUE).set_index(99)?;
                    }
                    if mode == 1 {
                        core::future::pending::<()>().await;
                    }
                    if mode == 2 {
                        return Err(capnp::Error::from_kind(capnp::ErrorKind::Failed));
                    }
                    Ok(())
                });
                if mode == 7 {
                    // A deliberately forgotten future keeps its mutable byte
                    // view. The pool must skip that buffer until scope teardown.
                    core::mem::forget(future);
                } else if mode != 6 {
                    let mut future = core::pin::pin!(future);
                    match future.as_mut().poll(&mut cx) {
                        Poll::Pending => assert_eq!(mode, 1),
                        Poll::Ready(Err(_)) => assert_eq!(mode, 2),
                        Poll::Ready(Ok(())) => assert!(!matches!(mode, 1 | 2 | 7)),
                    }
                    // A second buffer remains usable while output is borrowed.
                    let spare = messages.reserve(776).unwrap();
                    messages.discard(spare);
                }
            }
            if mode == 8 {
                let future = draft.write_async(messages, async |_, _| panic!("draft rebuilt"));
                let mut future = core::pin::pin!(future);
                assert_eq!(
                    future.as_mut().poll(&mut cx),
                    Poll::Ready(Err(DraftError::AlreadyStarted))
                );
            }
            if mode == 5 {
                draft.discard(messages, &mut peer.runtime);
            } else {
                match draft.finish(messages, &mut peer.runtime) {
                    Ok(result) => {
                        assert!(!matches!(mode, 1 | 2 | 3 | 6 | 7));
                        result
                            .with(messages, |payload| {
                                let pointer = payload
                                    .results(echo::Owned::CAPABILITY)
                                    .unwrap()
                                    .value()
                                    .unwrap();
                                assert_eq!(pointer.index(), if mode == 4 { 3 } else { 0 });
                                assert_eq!(payload.capability(pointer).unwrap().key(), expected);
                            })
                            .unwrap();
                        result.release(messages, &mut peer.runtime);
                    }
                    Err(DraftError::Incomplete) => assert!(matches!(mode, 1 | 2 | 6 | 7)),
                    Err(DraftError::Input(_)) => assert_eq!(mode, 3),
                    Err(other) => panic!("unexpected draft failure: {other:?}"),
                }
            }
            peer.runtime.release(target);
            assert_eq!(
                peer.drain_endpoint(),
                Some(Some(43)),
                "draft releases every capability before terminal cleanup"
            );
            assert_eq!(close(peer, messages, &mut queue), 0);
            let a = messages.reserve(776).unwrap();
            let b = messages.reserve(776).unwrap();
            if mode == 7 {
                assert!(matches!(
                    messages.reserve(776),
                    Err(messages::Error::NoBuffer)
                ));
            } else {
                let c = messages.reserve(776).unwrap();
                messages.discard(c);
            }
            messages.discard(a);
            messages.discard(b);
        });
    })
    .unwrap();
}
