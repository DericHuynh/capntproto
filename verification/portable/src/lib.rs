//! Generated borrowed APIs exercised with target features isolated from host tools.
#![no_std]
#![forbid(unsafe_code)]

capnp::generated_code!(pub mod portable_capnp);

use capnp::{message, ErrorKind, Result, Word};
use portable_capnp::{api::Record, record};

const ZERO: Word = capnp::word(0, 0, 0, 0, 0, 0, 0, 0);
mod server_output;
pub use server_output::bounded_server_output;
mod resource_output;
pub use resource_output::bounded_resource_output;
mod bindings;
mod bounded;
mod context_server;
mod provision;
pub use provision::bounded_provision_lifecycle;
mod provision_exchange;
pub use provision_exchange::bounded_provision_exchange;
mod network_driver;
pub use network_driver::{owning_network_registration, owning_provision_sender};
mod network_accept;
pub use network_accept::owning_network_acceptance;
mod network_rendezvous;
pub use network_rendezvous::owning_network_rendezvous;
mod network_await;
pub use network_await::owning_network_await;
mod network_join;
pub use network_join::owning_network_join;
mod network_vine;
pub use network_vine::owning_network_vine;
mod network_barrier;
pub use network_barrier::owning_network_barrier;
mod network_sender;
pub use network_sender::owning_adoption_sender;
mod network_adoption;
pub use network_adoption::{owning_adopted_capability, owning_network_adoption};
mod result_draft;
pub use result_draft::owned_result_draft;
mod session;
pub use session::typed_session_calls;
mod session_drive;
pub use session_drive::caller_polled_session;
mod streaming_pump;
#[cfg(feature = "growing")]
pub use streaming_pump::growing_binding_cleanup;
#[cfg(feature = "alloc")]
pub use streaming_pump::growing_streaming_acknowledgment_pump;
pub use streaming_pump::streaming_acknowledgment_pump;
#[cfg(feature = "alloc")]
pub use streaming_pump::{growing_ledger_cleanup, growing_ledger_streaming_acknowledgment_pump};
mod streaming;
pub use streaming::streaming_session_calls;
mod session_caps;
pub use session_caps::typed_capability_calls;
mod driver;
pub use context_server::{generated_context_nested_call, generated_context_wire_dispatch};
mod invocation_replies;
pub use invocation_replies::{queued_local_reply_bindings, queued_remote_reply_bindings};
mod local_result;
pub use local_result::local_reply_ownership;
mod driver_replies;
pub use driver_replies::tracked_reply_delivery;
mod driver_completion;
pub use driver_completion::caller_owned_reply_future;
mod driver_invocation;
mod driver_invocation_pipeline;
pub use driver_invocation::{owning_invocation_driver, owning_invocation_ordering};
pub use driver_invocation_pipeline::{local_invocation_pipeline, queued_invocation_pipeline};
mod driver_tail;
pub use driver_tail::{owning_tail_driver, owning_tail_pipeline};
mod driver_controls;
pub use driver_controls::owning_protocol_controls;
mod driver_termination;
pub use driver_termination::owning_terminal_cleanup;
mod driver_loopback;
pub use driver_loopback::{owning_loopback_driver, owning_loopback_unresolved};
mod embargo;
mod graph_connection;
pub use driver::{owning_connection_driver, owning_return_resolve_order};
mod owned_reflection;
pub use owned_reflection::owning_reflected_output;
mod exporting;
pub use exporting::exported_promise_failure;
mod outgoing_pipeline;
pub use outgoing_pipeline::outgoing_question_pipeline;
mod leased;
mod owning;
mod owning_failures;
mod pipeline;
mod promises;
mod publication;
mod reflected;
mod reflected_resolution;
mod reflected_return;
mod routing;
mod rpc;
mod server;
pub use graph_connection::{graph_connection, graph_resolution_retry};
pub use owning::owned_rpc_conversation;
pub use owning_failures::owned_task_failures;
mod tail;
pub use tail::bounded_tail_adoption;
mod wire_caps;
pub use bindings::bounded_imported_clients;
pub use bounded::{bounded_generic_fields, bounded_list_promotions, bounded_list_upgrades};
pub use embargo::bounded_loopback_ordering;
pub use leased::{
    leased_message_roundtrip, suspended_request_and_nested_call, suspended_result_and_nested_output,
};
pub use pipeline::bounded_pipeline_and_streaming;
pub use promises::bounded_promise_resolution;
pub use publication::bounded_rpc_publication;
pub use reflected::bounded_reflected_capabilities;
pub use reflected_resolution::bounded_reflected_resolution;
pub use reflected_return::bounded_reflected_return;
pub use routing::bounded_dispatch_ordering;
pub use rpc::generated_rpc_conversation;
pub use server::{generated_async_server, generated_branded_server};
pub use wire_caps::bounded_wire_capabilities;

/// Two bounded ownership tables exchange descriptor and Release metadata.
/// A server call lease outlives the client's last reference without allocation.
pub fn bounded_export_ownership() {
    use capntproto_rpc_engine::{
        exports::{with_exports, Slot as ExportSlot},
        ids::{ExportId, ImportId, WireId},
        imports::{with_imports, Slot as ImportSlot},
    };
    let mut server = [const { ExportSlot::EMPTY }; 2];
    let mut client = [const { ImportSlot::EMPTY }; 2];
    with_exports(&mut server, |exports| {
        with_imports(&mut client, |imports| {
            let mut batch = exports.batch();
            let grant = batch.export(41).unwrap();
            let id = grant.id();
            let key = grant.key();
            let repeated = batch.grant(key).unwrap();
            assert_eq!(repeated.id(), id);
            batch.commit();
            let import_id = ImportId::from_wire(id.to_wire());
            let first = imports.admit(import_id, 7).unwrap();
            let second = imports.receive(import_id).unwrap();
            let call = exports.lease(id).unwrap();
            {
                // A failed later message must not increase the peer's grant count.
                let mut unsent = exports.batch();
                let _ = unsent.grant(key).unwrap();
                let _ = unsent.export(99).unwrap();
            }
            assert_eq!(exports.take_retired(1), Some(99));
            imports.release(first);
            imports.release(second);
            let release = imports.pending_release(0).unwrap();
            assert_eq!(release.count(), 2);
            assert_eq!(
                exports.release(ExportId::from_wire(release.id().to_wire()), release.count()),
                Ok(None)
            );
            assert_eq!(release.commit(), 7);
            assert!(exports.resolve(key).is_none());
            assert_eq!(exports.get(&call), &41);
            assert_eq!(exports.release_local(call), Some(41));
            let mut batch = exports.batch();
            let reused = batch.export(43).unwrap();
            assert_eq!(reused.id(), id);
            batch.commit();
            assert_eq!(exports.resolve(key), None);
            assert_eq!(exports.release(id, 1), Ok(Some(43)));
            assert!(exports.is_empty() && imports.is_empty());
        })
        .unwrap();
    })
    .unwrap();
}

/// Local handle ownership and cleanup use caller storage, without an executor.
pub fn bounded_import_ownership() {
    use capntproto_rpc_engine::{
        ids::{ImportId, WireId},
        imports::{with_imports, Error, Slot},
        references::ExportRefs,
    };
    let mut storage = [const { Slot::EMPTY }; 2];
    let id = ImportId::from_wire(u32::MAX);
    with_imports(&mut storage, |imports| {
        let mut peer = ExportRefs::new();
        let first = imports.admit(id, 19).unwrap();
        let stale = first.key();
        let retained = imports.retain(&first).unwrap();
        peer.retain(1).unwrap();
        let received = imports.receive(id).unwrap();
        imports.release(first);
        imports.release(retained);
        assert_eq!(imports.get(&received), &19);
        assert!(imports.pending_release(0).is_none());
        imports.release(received);
        assert_eq!(imports.resolve(stale), None);
        for _ in 0..3 {
            // Rejecting output does not lose cleanup or make the slot reusable.
            let release = imports.pending_release(0).unwrap();
            assert_eq!(release.count(), peer.count());
        }
        assert_eq!(imports.admit(id, 99).unwrap_err().error, Error::Duplicate);
        let revived = imports.receive(id).unwrap();
        peer.retain(1).unwrap();
        assert_eq!(imports.resolve(stale), None);
        imports.release(revived);
        let release = imports.pending_release(0).unwrap();
        assert_eq!(release.id(), id);
        assert_eq!(peer.release(release.count()), Ok(true));
        assert_eq!(release.commit(), 19);
        assert!(imports.is_empty());
        let reused = imports.admit(id, 23).unwrap();
        assert_eq!(imports.resolve(stale), None);
        imports.release(reused);
        assert_eq!(imports.pending_release(0).unwrap().commit(), 23);
    })
    .unwrap();
}

/// Wire-reference transitions require neither an allocator nor a host executor.
pub fn bounded_capability_references() {
    use capntproto_rpc_engine::references::{Error, ExportRefs, ImportRefs};
    let mut export = ExportRefs::new();
    let mut import = ImportRefs::new();
    import.receive(1).unwrap();
    for _ in 0..8 {
        export.retain(1).unwrap();
        import.receive(1).unwrap();
        // Simulate an output adapter rejecting cleanup without losing it.
        assert_eq!(import.pending_release().unwrap().count(), export.count());
    }
    assert_eq!(export.release(10), Err(Error::OverRelease));
    assert_eq!(export.count(), 9);
    let release = import.pending_release().unwrap();
    assert_eq!(export.release(release.count()), Ok(true));
    release.commit();
    assert!(import.pending_release().is_none());
    assert_eq!(export.retain(1), Err(Error::Released));
    import.receive(u32::MAX).unwrap();
    assert_eq!(import.receive(1), Err(Error::Overflow));
    assert_eq!(import.pending_release().unwrap().count(), u32::MAX);
    import.pending_release().unwrap().commit();
    assert_eq!(import.count(), 0);
}

/// Uses the same generated source as the hosted API, including generic fields.
pub fn fixed_buffer_views_and_edits() -> Result<()> {
    let mut storage = [ZERO; 64];
    {
        let allocator =
            message::SingleSegmentAllocator::new(Word::words_to_bytes_mut(&mut storage));
        let mut message = message::Builder::new(allocator);
        {
            let mut raw = message.init_root::<record::Builder<'_>>();
            raw.set_label("hello");
            raw.set_payload(b"abc");
            let mut numbers = raw.reborrow().init_numbers(2);
            numbers.set(0, 3);
            numbers.set(1, 5);
            raw.reborrow().init_children(1).get(0).set_id(19);
            raw.init_nested().set_value("nested")?;
        }
        let used = message.get_segments_for_output()[0].len();
        {
            let mut editor = message.get_root::<record::Builder<'_>>()?.into_api();
            editor.count().set(7);
            editor
                .label()
                .edit()?
                .as_bytes_mut()
                .copy_from_slice(b"world");
            editor.payload().edit()?[1] = b'Z';
            editor.numbers().edit()?.get_mut(1).unwrap().set(11);
            editor.children().edit()?.get_mut(0).unwrap().id().set(23);
            editor.nested().edit()?.value().edit()?.as_bytes_mut()[0] = b'N';
            assert_eq!(
                editor.details().edit().err().unwrap().kind,
                ErrorKind::NotPresent
            );
            let mut details = editor.details().ensure();
            details.enabled().set(true);
            details.value().set(91);
        }
        assert_eq!(message.get_segments_for_output()[0].len(), used);
        let reader = message
            .get_root_as_reader::<record::Reader<'_>>()?
            .into_api();
        assert_eq!(reader.count(), 7);
        assert_eq!(reader.label()?, "world");
        assert_eq!(reader.payload()?, b"aZc");
        let numbers = reader.numbers()?;
        assert_eq!(numbers.iter().sum::<u32>(), 14);
        assert_eq!(numbers.get(usize::MAX), None);
        assert_eq!(numbers.as_slice(), Some(&[3, 11][..]));
        assert_eq!(reader.children()?.get(0).unwrap().id(), 23);
        assert_eq!(reader.nested()?.value()?.to_str()?, "Nested");
        assert!(reader.details()?.enabled());
        assert_eq!(reader.details()?.value(), 91);
        let payload = reader.payload()?;
        let segment = message.get_segments_for_output();
        assert!(payload.as_ptr() >= segment[0].as_ptr());
        assert!(payload.as_ptr_range().end <= segment[0].as_ptr_range().end);

        // Caller-owned framing storage and segment index round-trip borrowed views.
        let mut wire = [ZERO; 65];
        let bytes = Word::words_to_bytes_mut(&mut wire);
        let mut output = &mut bytes[..];
        capnp::serialize::write_message(&mut output, &message)?;
        let remaining = output.len();
        let mut input = &bytes[..bytes.len() - remaining];
        let mut index = [capnp::serialize::SegmentOffset::EMPTY; 1];
        let segments = capnp::serialize::IndexedSliceSegments::from_slice(
            &mut input,
            &mut index,
            message::ReaderOptions::new(),
        )?;
        let decoded = message::Reader::new(segments, message::ReaderOptions::new());
        assert_eq!(
            decoded
                .get_root::<record::Reader<'_>>()?
                .into_api()
                .label()?,
            "world"
        );
        assert!(input.is_empty());
    }
    // Returning storage zeroes it for reuse, without a heap owner.
    assert!(storage.iter().all(|word| *word == ZERO));
    Ok(())
}

pub fn defaults_and_union_rejection() -> Result<()> {
    let mut storage = [ZERO; 16];
    let allocator = message::SingleSegmentAllocator::new(Word::words_to_bytes_mut(&mut storage));
    let mut message = message::Builder::new(allocator);
    message.init_root::<record::Builder<'_>>();
    let reader = message
        .get_root_as_reader::<record::Reader<'_>>()?
        .into_api();
    assert_eq!(reader.count(), 42);
    assert_eq!(reader.label()?, "unassigned");
    assert!(reader.field(Record::LABEL).is_null());
    assert_eq!(reader.field(Record::LABEL).present()?, None);
    assert_eq!(
        reader.field(Record::LABEL).wire_text()?.to_str()?,
        "unassigned"
    );
    assert!(reader.numbers()?.is_empty());
    assert_eq!(reader.details().err().unwrap().kind, ErrorKind::NotPresent);
    let mut editor = message.get_root::<record::Builder<'_>>()?.into_api();
    assert_eq!(
        editor.label().edit().err().unwrap().kind,
        ErrorKind::NotPresent
    );
    assert!(matches!(
        editor.label().entry()?,
        capnp::field_api::Entry::Vacant(_)
    ));
    // Explicit ensure can construct a default using the supplied fixed arena.
    assert_eq!(editor.label().ensure()?.as_bytes(), b"unassigned");
    Ok(())
}

pub fn malformed_borrowed_field() -> Result<()> {
    let words = [
        capnp::word(0, 0, 0, 0, 0, 0, 1, 0), // Root: no data, one pointer.
        capnp::word(1, 0, 0, 0, 18, 0, 0, 0), // Two-byte text outside the segment.
    ];
    let segments = [Word::words_to_bytes(&words)];
    let message = message::Reader::new(&segments[..], message::ReaderOptions::new());
    let reader = message.get_root::<record::Reader<'_>>()?.into_api();
    assert_eq!(reader.count(), 42); // Scalar defaults do not traverse the bad pointer.
    assert_eq!(
        reader.label().unwrap_err().kind,
        ErrorKind::MessageContainsOutOfBoundsPointer
    );
    assert_eq!(
        reader.field(Record::LABEL).present().unwrap_err().kind,
        ErrorKind::MessageContainsOutOfBoundsPointer
    );
    Ok(())
}

pub fn invalid_utf8_preserves_wire_access() -> Result<()> {
    let words = [
        capnp::word(0, 0, 0, 0, 0, 0, 1, 0),
        capnp::word(1, 0, 0, 0, 18, 0, 0, 0),
        capnp::word(255, 0, 0, 0, 0, 0, 0, 0),
    ];
    let segments = [Word::words_to_bytes(&words)];
    let message = message::Reader::new(&segments[..], message::ReaderOptions::new());
    let reader = message.get_root::<record::Reader<'_>>()?.into_api();
    assert!(matches!(
        reader.label().unwrap_err().kind,
        ErrorKind::TextContainsNonUtf8Data(_)
    ));
    assert_eq!(reader.field(Record::LABEL).wire_text()?.as_bytes(), &[255]);
    Ok(())
}

pub fn caller_supplied_diagnostics() {
    use core::fmt::Write;
    struct Buffer {
        bytes: [u8; 64],
        len: usize,
    }
    impl Write for Buffer {
        fn write_str(&mut self, value: &str) -> core::fmt::Result {
            let end = self.len.checked_add(value.len()).ok_or(core::fmt::Error)?;
            let output = self.bytes.get_mut(self.len..end).ok_or(core::fmt::Error)?;
            output.copy_from_slice(value.as_bytes());
            self.len = end;
            Ok(())
        }
    }
    let mut buffer = Buffer {
        bytes: [0; 64],
        len: 0,
    };
    use capnp::field_api::diagnostics::Location;
    let location = Location::Field {
        schema: "Record",
        name: "label",
        ordinal: Some(1),
    };
    write!(
        buffer,
        "{location}{} {}",
        Location::Index(3),
        Location::Union("Record")
    )
    .unwrap();
    assert_eq!(
        &buffer.bytes[..buffer.len],
        b"Record.label (@1)[3] Record union"
    );
    let error = location.error(capnp::Error::from_kind(ErrorKind::NotPresent));
    assert_eq!(error.kind, ErrorKind::NotPresent);
    #[cfg(feature = "alloc")]
    assert_eq!(error.extra, "Record.label (@1)");

    use capntproto_rpc_engine::{
        ids::{ImportId, WireId},
        peer_table::{BorrowedSparse, Slot, SparseStorage},
    };
    struct OpaqueOwner;
    let mut slots = [const { Slot::EMPTY }; 2];
    let mut backing = BorrowedSparse::new(&mut slots);
    assert!(backing
        .try_insert(ImportId::from_wire(17), OpaqueOwner)
        .is_ok());
    buffer.len = 0;
    write!(buffer, "{backing:?}").unwrap();
    assert_eq!(
        &buffer.bytes[..buffer.len],
        b"BorrowedSparse { len: 1, capacity: 2 }"
    );
    assert_eq!(backing.len(), 1);
    use capntproto_rpc_engine::local_table::{BorrowedSlots, Slot as LocalSlot};
    let mut local = [LocalSlot::<OpaqueOwner>::EMPTY];
    let local = BorrowedSlots::new(&mut local);
    buffer.len = 0;
    write!(buffer, "{local:?}").unwrap();
    assert_eq!(
        &buffer.bytes[..buffer.len],
        b"BorrowedSlots { capacity: 1 }"
    );
}

/// Peer IDs consume fixed caller capacity, irrespective of their wire value.
pub fn bounded_rpc_tables() {
    use capntproto_rpc_engine::{
        ids::{ImportId, WireId},
        peer_table::{BorrowedSparse, InsertErrorKind, PeerTable, Slot},
    };
    let mut storage = [const { Slot::EMPTY }; 2];
    for _ in 0..2 {
        let mut table =
            PeerTable::<ImportId, u32, _, 1>::try_new(BorrowedSparse::new(&mut storage))
                .expect("borrowed slots were not cleared");
        for raw in [0, 1, u32::MAX] {
            table
                .try_insert_unique(ImportId::from_wire(raw), raw)
                .unwrap();
        }
        let full = table
            .try_insert_unique(ImportId::from_wire(2), 42)
            .unwrap_err();
        assert_eq!(full.kind, InsertErrorKind::Full);
        assert_eq!(full.value, 42);
        let duplicate = table
            .try_insert_unique(ImportId::from_wire(u32::MAX), 19)
            .unwrap_err();
        assert_eq!(duplicate.kind, InsertErrorKind::Duplicate);
        assert_eq!(table.get(&ImportId::from_wire(u32::MAX)), Some(&u32::MAX));
        assert_eq!(table.remove(&ImportId::from_wire(1)), Some(1));
        assert_eq!(table.remove(&ImportId::from_wire(1)), None);
        table
            .try_insert_unique(ImportId::from_wire(2), full.value)
            .unwrap();
        assert_eq!(
            table.try_insert(ImportId::from_wire(u32::MAX), 9),
            Ok(Some(u32::MAX))
        );
        *table.get_mut(&ImportId::from_wire(0)).unwrap() = 7;
        for value in table.values_mut() {
            *value += 1;
        }
        assert_eq!(table.values().copied().sum::<u32>(), 61);
        assert_eq!(table.len(), 3);
        assert!(!table.is_empty());
    }
}

/// Locally chosen IDs use bounded caller memory for values and their free heap.
pub fn bounded_local_rpc_tables() {
    use capntproto_rpc_engine::{
        ids::{QuestionId, WireId},
        local_table::{BorrowedSlots, InsertErrorKind, LocalTable, Slot},
        peer_table::{BorrowedSparse, Slot as SparseSlot},
    };
    let mut dense = [const { Slot::EMPTY }; 4];
    let mut high = [SparseSlot::EMPTY];
    let mut adopted = [SparseSlot::EMPTY];
    for _ in 0..2 {
        let mut table = LocalTable::<QuestionId, _, _, _, _>::try_new(
            BorrowedSlots::new(&mut dense),
            BorrowedSparse::new(&mut high),
            BorrowedSparse::new(&mut adopted),
        )
        .expect("empty local storage");
        for raw in 0..4 {
            assert_eq!(table.try_push(raw).unwrap().to_wire(), raw);
        }
        let full = table.try_push(42).unwrap_err();
        assert_eq!(full.kind, InsertErrorKind::Full);
        for raw in [3, 1, 0] {
            assert_eq!(table.remove(QuestionId::from_wire(raw)), Some(raw));
            assert_eq!(table.remove(QuestionId::from_wire(raw)), None);
        }
        for raw in [0, 1, 3] {
            assert_eq!(table.try_push(full.value).unwrap().to_wire(), raw);
        }
        let id = table.try_push_high(8).unwrap();
        assert_eq!(id.to_wire(), 1 << 31);
        assert_eq!(
            table.try_push_high(9).unwrap_err().kind,
            InsertErrorKind::Full
        );
        assert_eq!(table.remove(id), Some(8));
        assert_eq!(table.try_push_high(10).unwrap().to_wire(), (1 << 31) + 1);
        let adopted = QuestionId::from_wire((1 << 31) - 1);
        table.try_insert_adopted(adopted, 11).unwrap();
        assert_eq!(
            table.try_insert_adopted(adopted, 12).unwrap_err().kind,
            InsertErrorKind::Duplicate
        );
        assert_eq!(
            table.try_insert_adopted(id, 13).unwrap_err().kind,
            InsertErrorKind::InvalidAdoptedId
        );
        assert_eq!(
            table
                .try_insert_adopted(QuestionId::from_wire(1 << 30), 14)
                .unwrap_err()
                .kind,
            InsertErrorKind::Full
        );
        *table.get_mut(adopted).unwrap() += 1;
        assert_eq!(table.get(adopted), Some(&12));
        assert_eq!(table.len(), 6);
        assert_eq!(table.values().count(), 6);
    }
}

/// Application handoff guards share the hosted implementation and require no IO.
pub fn bounded_handoff_transitions() {
    use capntproto_rpc_engine::handoff::{HandoffPhase, HandoffState};
    let mut state = HandoffState::default();
    assert!(state.enqueue());
    assert!(state.provide());
    assert!(!state.accept(false));
    assert!(state.accept(true));
    assert!(!state.lift());
    assert!(state.drain());
    assert!(state.lift());
    assert_eq!(state.phase(), HandoffPhase::Direct);
    assert!(state.invoke_direct());
    assert_eq!(state.direct_calls(), 1);
    assert!(state.revoke());
    assert!(!state.invoke_direct());
    assert!(!state.accept(true));
}

/// Framing can stop between any reads while retaining progress in caller memory.
pub fn resumable_stream_framing() -> Result<()> {
    use capnp::serialize::FrameReceiver;
    let mut storage = [ZERO; 32];
    let allocator = message::SingleSegmentAllocator::new(Word::words_to_bytes_mut(&mut storage));
    let mut builder = message::Builder::new(allocator);
    let mut record = builder.init_root::<record::Builder<'_>>();
    record.set_count(17);
    record.set_label("partial IO");
    let mut wire = [ZERO; 33];
    let bytes = Word::words_to_bytes_mut(&mut wire);
    let mut output = &mut bytes[..];
    capnp::serialize::write_message(&mut output, &builder)?;
    let remaining = output.len();
    let wire = &bytes[..bytes.len() - remaining];

    let mut buffer = [ZERO; 33];
    let mut receiver =
        FrameReceiver::new(Word::words_to_bytes_mut(&mut buffer), Default::default())?;
    for _ in 0..2 {
        for &byte in wire {
            let progress = receiver.received_bytes();
            {
                let _pending = receiver.input()?.unwrap(); // A cancelled poll commits nothing.
            }
            assert_eq!(receiver.received_bytes(), progress);
            let mut window = receiver.input()?.unwrap();
            window.bytes()[0] = byte;
            window.commit(1)?;
        }
        assert!(receiver.input()?.is_none());
        let reader = receiver.message()?.unwrap();
        let record = reader.get_root::<record::Reader<'_>>()?.into_api();
        assert_eq!(record.count(), 17);
        assert_eq!(record.label()?, "partial IO");
        receiver.next_message()?;
    }
    receiver.input()?.unwrap().commit(0)?;
    assert!(receiver.into_message()?.is_none());

    // On 32-bit targets these separately overflow the sum, multiplication and
    // table addition. On 64-bit targets the same frames exceed caller capacity.
    for header in [
        [
            capnp::word(1, 0, 0, 0, 255, 255, 255, 255),
            capnp::word(1, 0, 0, 0, 0, 0, 0, 0),
        ],
        [capnp::word(0, 0, 0, 0, 0, 0, 0, 32), ZERO],
        [capnp::word(0, 0, 0, 0, 255, 255, 255, 31), ZERO],
    ] {
        let mut options = message::ReaderOptions::new();
        options.traversal_limit_in_words = None;
        let mut receiver = FrameReceiver::new(Word::words_to_bytes_mut(&mut buffer), options)?;
        let mut input = Word::words_to_bytes(&header);
        let error = loop {
            let mut window = receiver.input()?.unwrap();
            let n = window.bytes().len();
            window.bytes().copy_from_slice(&input[..n]);
            input = &input[n..];
            if let Err(error) = window.commit(n) {
                break error;
            }
        };
        let expected = if usize::BITS == 32 {
            ErrorKind::MessageSizeOverflow
        } else {
            ErrorKind::BufferNotLargeEnough
        };
        assert_eq!(error.kind, expected);
        assert_eq!(receiver.message().err().unwrap().kind, expected);
        assert_eq!(receiver.input().err().unwrap().kind, expected);
    }
    Ok(())
}

/// Every growth request either reserves first or keeps the old field and tag.
pub fn fallible_fixed_construction() -> Result<()> {
    use portable_capnp::api::{BoundedRecord, Child};
    let mut storage = [ZERO; 24];
    {
        let mut message = message::FixedBuilder::new(&mut storage)?;
        {
            let mut root = message.try_init_root::<BoundedRecord>()?;
            assert_eq!(root.read().id(), 42);
            assert_eq!(root.read().title()?, "default");
            root.scalar(BoundedRecord::ID).set(23);
            root.field(BoundedRecord::TITLE).copy_from("bounded")?;
            root.field(BoundedRecord::PAYLOAD)
                .init(3)?
                .copy_from_slice(b"abc");
            root.field(BoundedRecord::CHILD)
                .init()?
                .scalar(Child::ID)
                .set(91);
            root.scalar(BoundedRecord::NUMBER).set(7);
            let oversized = [b'x'; 256];
            assert!(root
                .field(BoundedRecord::NOTE)
                .copy_from(core::str::from_utf8(&oversized).unwrap())
                .is_err());
            assert_eq!(root.read().number()?, 7);
            assert_eq!(root.read().note().unwrap_err().kind, ErrorKind::NotPresent);
        }
        let mut reference_storage = [ZERO; 24];
        let mut reference = message::Builder::new(message::SingleSegmentAllocator::new(
            Word::words_to_bytes_mut(&mut reference_storage),
        ));
        let mut root = reference.init_root::<portable_capnp::bounded_record::Builder<'_>>();
        root.set_id(23);
        root.set_title("bounded");
        root.set_payload(b"abc");
        root.reborrow().init_child().set_id(91);
        root.set_number(7);
        assert_eq!(
            message.get_segments_for_output()[0],
            reference.get_segments_for_output()[0]
        );

        {
            let mut root = message.try_get_root::<BoundedRecord>()?;
            root.field(BoundedRecord::NOTE).copy_from("selected")?;
            assert_eq!(root.read().note()?, "selected");
            root.field(BoundedRecord::LINKED)
                .replace()?
                .scalar(Child::ID)
                .set(18);
            assert_eq!(root.read().linked()?.id(), 18);
            assert_eq!(root.read().note().unwrap_err().kind, ErrorKind::NotPresent);
            root.scalar(BoundedRecord::NUMBER).set(7);
            assert_eq!(root.read().number()?, 7);
            assert_eq!(root.read().title()?, "bounded");
        }

        let available = message.remaining_words();
        message
            .try_get_root::<BoundedRecord>()?
            .field(BoundedRecord::PAYLOAD)
            .replace(available as usize * 8)?
            .fill(0xa5);
        assert_eq!(message.remaining_words(), 0);
        let mut snapshot = [0; 24 * 8];
        snapshot.copy_from_slice(message.get_segments_for_output()[0]);
        assert!(matches!(
            message.try_init_root::<BoundedRecord>().err().unwrap().kind,
            ErrorKind::InsufficientCapacity { .. }
        ));
        {
            let mut root = message.try_get_root::<BoundedRecord>()?;
            assert!(matches!(
                root.field(BoundedRecord::NOTE)
                    .copy_from("")
                    .unwrap_err()
                    .kind,
                ErrorKind::InsufficientCapacity { .. }
            ));
            assert!(matches!(
                root.field(BoundedRecord::LINKED)
                    .replace()
                    .err()
                    .unwrap()
                    .kind,
                ErrorKind::InsufficientCapacity { .. }
            ));
            assert!(matches!(
                root.field(BoundedRecord::CHILD)
                    .replace()
                    .err()
                    .unwrap()
                    .kind,
                ErrorKind::InsufficientCapacity { .. }
            ));
            assert_eq!(
                root.field(BoundedRecord::PAYLOAD)
                    .replace(usize::MAX)
                    .err()
                    .unwrap()
                    .kind,
                ErrorKind::LengthOverflow
            );
            assert_eq!(
                root.field(BoundedRecord::PAYLOAD)
                    .init(1)
                    .err()
                    .unwrap()
                    .kind,
                ErrorKind::AlreadyPresent
            );
            assert_eq!(root.read().id(), 23);
            assert_eq!(root.read().title()?, "bounded");
            assert_eq!(root.read().child()?.id(), 91);
            assert_eq!(root.read().number()?, 7);
        }
        assert_eq!(message.size_in_words(), 24);
        assert_eq!(message.get_segments_for_output()[0], snapshot);
        // Existing in-place edits need no reservation, including at exhaustion.
        message
            .try_get_root::<BoundedRecord>()?
            .field(BoundedRecord::CHILD)
            .edit()?
            .scalar(Child::ID)
            .set(92);
        message
            .try_get_root::<BoundedRecord>()?
            .field(BoundedRecord::PAYLOAD)
            .edit()?[0] = 3;
        assert_eq!(message.size_in_words(), 24);
        message.reset();
        assert_eq!(
            message.try_get_root::<BoundedRecord>().err().unwrap().kind,
            ErrorKind::NotPresent
        );
        assert_eq!(message.try_init_root::<BoundedRecord>()?.read().id(), 42);
    }
    assert!(storage.iter().all(|word| *word == ZERO));
    Ok(())
}

/// Nested list editors retain the bounded API, including structs and open enums.
pub fn bounded_lists_and_groups() -> Result<()> {
    use portable_capnp::api::{
        collections::{Details, Plain},
        Collections, Entry, State,
    };
    let mut words = [ZERO; 96];
    let mut message = message::FixedBuilder::new(&mut words)?;
    {
        let mut root = message.try_init_root::<Collections>()?;
        root.scalar(Collections::SIBLING).set(73);
        let mut bits = root.field(Collections::BITS).init(9)?;
        bits.get_mut(0).unwrap().set(true);
        bits.get_mut(8).unwrap().set(true);
        assert!(bits.get_mut(9).is_none());
        assert!(bits.get_mut(usize::MAX).is_none());
        let mut numbers = root.field(Collections::NUMBERS).init(2)?;
        numbers.get_mut(1).unwrap().set(u64::MAX);
        let mut names = root.field(Collections::NAMES).init(2)?;
        names.get_mut(0).unwrap().copy_from("first")?;
        names.get_mut(1).unwrap().copy_from("second")?;
        root.field(Collections::CHUNKS)
            .init(1)?
            .get_mut(0)
            .unwrap()
            .copy_from(&[1, 2, 3])?;
        let mut entries = root.field(Collections::ENTRIES).init(2)?;
        let mut entry = entries.get_mut(1).unwrap();
        entry.scalar(Entry::ID).set(91);
        entry.field(Entry::LABEL).copy_from("nested")?;
        let mut matrix = root.field(Collections::MATRIX).init(1)?;
        matrix
            .get_mut(0)
            .unwrap()
            .init(2)?
            .get_mut(1)
            .unwrap()
            .set(65535);
        root.field(Collections::STATES)
            .init(1)?
            .get_mut(0)
            .unwrap()
            .set(State::Unknown(65534));
        assert_eq!(
            root.group(Collections::DETAILS).edit().err().unwrap().kind,
            ErrorKind::NotPresent
        );
        let mut details = root.group(Collections::DETAILS).select();
        details.scalar(Details::VALUE).set(19);
        details.field(Details::LABEL).copy_from("group")?;
        let mut plain = root.group(Collections::PLAIN).edit()?;
        assert_eq!(plain.read().value(), 9);
        plain.scalar(Plain::VALUE).set(42);
        plain.field(Plain::LABEL).copy_from("sibling group")?;
    }
    // Exact bytes match the hosted encoder with the same allocation order.
    let mut reference_words = [ZERO; 96];
    let mut reference = message::Builder::new(message::SingleSegmentAllocator::new(
        Word::words_to_bytes_mut(&mut reference_words),
    ));
    {
        let mut root = reference.init_root::<portable_capnp::collections::Builder<'_>>();
        root.set_sibling(73);
        let mut bits = root.reborrow().init_bits(9);
        bits.set(0, true);
        bits.set(8, true);
        root.reborrow().init_numbers(2).set(1, u64::MAX);
        let mut names = root.reborrow().init_names(2);
        names.set(0, "first");
        names.set(1, "second");
        root.reborrow().init_chunks(1).set(0, &[1, 2, 3]);
        let mut entry = root.reborrow().init_entries(2).get(1);
        entry.set_id(91);
        entry.set_label("nested");
        root.reborrow().init_matrix(1).init(0, 2).set(1, 65535);
        // Legacy enum setters reject unknown ordinals; edit its primitive wire view.
        let mut states = root.reborrow().init_states(1);
        states.reborrow().set(0, portable_capnp::State::Waiting);
        let mut details = root.reborrow().init_details();
        details.set_value(19);
        details.set_label("group");
        let mut plain = root.get_plain();
        plain.set_value(42);
        plain.set_label("sibling group");
    }
    // Set a known ordinal for the byte comparison, then restore the open value.
    message
        .try_get_root::<Collections>()?
        .field(Collections::STATES)
        .edit()?
        .get_mut(0)
        .unwrap()
        .set(State::Waiting);
    assert_eq!(
        message.get_segments_for_output()[0],
        reference.get_segments_for_output()[0]
    );
    message
        .try_get_root::<Collections>()?
        .field(Collections::STATES)
        .edit()?
        .get_mut(0)
        .unwrap()
        .set(State::Unknown(65534));
    let before = message.size_in_words();
    {
        let mut root = message.try_get_root::<Collections>()?;
        assert_eq!(root.read().states()?.get(0), Some(State::Unknown(65534)));
        assert_eq!(root.read().matrix()?.get(0).unwrap()?.get(1), Some(65535));
        assert_eq!(root.read().entries()?.get(1).unwrap().label()?, "nested");
        assert_eq!(
            root.field(Collections::ENTRIES)
                .replace(usize::MAX)
                .err()
                .unwrap()
                .kind,
            ErrorKind::LengthOverflow
        );
        assert!(matches!(
            root.field(Collections::SELECTED)
                .replace(1000)
                .err()
                .unwrap()
                .kind,
            ErrorKind::InsufficientCapacity { .. }
        ));
        assert_eq!(root.read().details()?.value(), 19);
        root.group(Collections::PLAIN).reset();
        assert_eq!(root.read().plain().value(), 9);
        assert_eq!(root.read().plain().label()?, "");
        assert_eq!(root.read().details()?.label()?, "group");
        root.group(Collections::DETAILS).reset();
        assert_eq!(root.read().details()?.value(), 0);
        assert_eq!(root.read().sibling(), 73);
        root.void(Collections::IDLE).set();
        assert_eq!(
            root.group(Collections::DETAILS).edit().err().unwrap().kind,
            ErrorKind::NotPresent
        );
    }
    assert_eq!(message.size_in_words(), before);
    // Struct-list edit may not silently promote a primitive encoding.
    message.reset();
    message
        .try_init_root_list::<u64>(2)?
        .get_mut(0)
        .unwrap()
        .set(17);
    assert_eq!(
        message.try_get_root_list::<Entry>().err().unwrap().kind,
        ErrorKind::NeedsUpgrade
    );
    assert_eq!(message.try_get_root_list::<u64>()?.read().get(0), Some(17));
    Ok(())
}

/// Fail at every reservation boundary of a recursive tree, then retry in place.
pub fn recursive_fixed_copies() -> Result<()> {
    use capnp::field_api::{Data, List, Text};
    let mut source_words = [ZERO; 32];
    let mut source = message::FixedBuilder::new(&mut source_words)?;
    let mut outer = source.try_init_root_list::<List<Text>>(2)?;
    let mut first = outer.get_mut(0).unwrap().init(2)?;
    first.get_mut(0).unwrap().copy_from("alpha")?;
    first.get_mut(1).unwrap().copy_from("beta")?;
    outer
        .get_mut(1)
        .unwrap()
        .init(1)?
        .get_mut(0)
        .unwrap()
        .copy_from("gamma")?;
    let expected_words = source.size_in_words() + 2; // old text/root + staging slot + copied body
    let reader = source.get_root_as_reader::<capnp::any_pointer::Reader<'_>>()?;
    for capacity in 2..=expected_words + 1 {
        let mut dest_words = [ZERO; 32];
        let mut dest = message::FixedBuilder::new(&mut dest_words[..capacity])?;
        dest.try_set_root_text("old")?;
        for _ in 0..2 {
            let result = dest.try_copy_root(reader);
            if capacity < expected_words {
                assert!(matches!(
                    result.unwrap_err().kind,
                    ErrorKind::InsufficientCapacity { .. }
                ));
                assert_eq!(dest.size_in_words(), 2);
                assert_eq!(dest.get_root_as_reader::<capnp::text::Reader<'_>>()?, "old");
            } else {
                result?;
                assert_eq!(dest.size_in_words(), expected_words);
                assert_eq!(
                    dest.get_root_as_reader::<capnp::any_pointer::Reader<'_>>()?
                        .equals(reader)?,
                    capnp::Equality::Equal
                );
                break;
            }
        }
    }
    // An invalid later child must roll back earlier successful child copies.
    let malformed = [
        capnp::word(1, 0, 0, 0, 22, 0, 0, 0),
        capnp::word(5, 0, 0, 0, 18, 0, 0, 0),
        capnp::word(9, 0, 0, 0, 10, 0, 0, 0),
        capnp::word(b'a', 0, 0, 0, 0, 0, 0, 0),
    ];
    let segments = [Word::words_to_bytes(&malformed)];
    let bad = message::Reader::new(&segments[..], message::ReaderOptions::new());
    let mut dest_words = [ZERO; 32];
    let mut dest = message::FixedBuilder::new(&mut dest_words)?;
    dest.try_set_root_text("old")?;
    assert!(dest.try_copy_root(bad.get_root()?).is_err());
    assert_eq!(dest.size_in_words(), 2);
    assert_eq!(dest.get_root_as_reader::<capnp::text::Reader<'_>>()?, "old");
    let cycle = [
        capnp::word(0, 0, 0, 0, 0, 0, 1, 0),
        capnp::word(0xfc, 0xff, 0xff, 0xff, 0, 0, 1, 0),
    ];
    let segments = [Word::words_to_bytes(&cycle)];
    let mut options = message::ReaderOptions::new();
    options.nesting_limit = 2;
    let cycle = message::Reader::new(&segments[..], options);
    assert_eq!(
        dest.try_copy_root(cycle.get_root()?).unwrap_err().kind,
        ErrorKind::MessageIsTooDeeplyNestedOrContainsCycles
    );
    assert_eq!(dest.size_in_words(), 2);
    // Capability rejection stays identical regardless of the alloc feature.
    let cap = [capnp::word(3, 0, 0, 0, 0, 0, 0, 0)];
    let segments = [Word::words_to_bytes(&cap)];
    let cap = message::Reader::new(&segments[..], message::ReaderOptions::new());
    assert_eq!(
        dest.try_copy_root(cap.get_root()?).unwrap_err().kind,
        ErrorKind::Unimplemented
    );
    assert_eq!(dest.size_in_words(), 2);
    dest.reset();
    dest.try_init_root_list::<Data>(0)?;
    assert!(dest.try_get_root_list::<Data>()?.is_empty());
    Ok(())
}

/// Upgrading moves existing payloads and preserves fields unknown to old code.
pub fn bounded_struct_upgrades() -> Result<()> {
    use capnp::field_api::Schema;
    use portable_capnp::api::{Entry, EntryV2};
    let old_size = 1 + Entry::SIZE.total() as usize + 1;
    let upgraded_size = old_size + 1 + EntryV2::SIZE.total() as usize;
    for capacity in old_size..=upgraded_size {
        let mut words = [ZERO; 32];
        let mut message = message::FixedBuilder::new(&mut words[..capacity])?;
        let mut entry = message.try_init_root::<Entry>()?;
        entry.scalar(Entry::ID).set(31);
        entry.field(Entry::LABEL).copy_from("live")?;
        let pointer = entry.read().label()?.as_ptr();
        assert_eq!(
            message.try_get_root::<EntryV2>().err().unwrap().kind,
            ErrorKind::NeedsUpgrade
        );
        let result = message.try_upgrade_root::<EntryV2>();
        if capacity < upgraded_size {
            assert!(matches!(
                result.err().unwrap().kind,
                ErrorKind::InsufficientCapacity { .. }
            ));
            assert_eq!(message.size_in_words(), old_size);
        } else {
            let mut newer = result?;
            assert_eq!(newer.read().extra(), 17);
            newer.scalar(EntryV2::EXTRA).set(123);
            assert_eq!(newer.read().label()?.as_ptr(), pointer);
            assert_eq!(message.size_in_words(), upgraded_size);
            message
                .try_upgrade_root::<Entry>()?
                .scalar(Entry::ID)
                .set(32);
            assert_eq!(message.size_in_words(), upgraded_size);
            assert_eq!(message.try_get_root::<EntryV2>()?.read().extra(), 123);
        }
        assert_eq!(message.try_get_root::<Entry>()?.read().label()?, "live");
    }
    Ok(())
}

/// Linked into the bare-metal binary so target compilation includes each path.
pub fn exercise_generated_api() -> Result<()> {
    bounded_resource_output();
    bounded_server_output();
    for mode in 0..6 {
        owning_network_vine(mode);
    }
    owning_network_rendezvous(false);
    owning_network_rendezvous(true);
    for mode in 0..16 {
        owning_network_await(mode);
    }
    for mode in 0..6 {
        owning_network_join(mode);
    }
    for mode in 0..5 {
        owning_network_barrier(mode);
    }
    for mode in 0..3 {
        owning_adoption_sender(mode);
    }
    for mode in 0..3 {
        owning_adopted_capability(mode);
    }
    for mode in 0..8 {
        owning_network_adoption(mode);
    }
    owning_provision_sender();
    bounded_provision_lifecycle();
    bounded_provision_exchange(false);
    bounded_provision_exchange(true);
    for mode in 0..5 {
        owning_network_registration(mode);
    }
    for mode in 0..7 {
        owning_network_acceptance(mode);
    }
    fixed_buffer_views_and_edits()?;
    defaults_and_union_rejection()?;
    malformed_borrowed_field()?;
    invalid_utf8_preserves_wire_access()?;
    caller_supplied_diagnostics();
    bounded_rpc_tables();
    bounded_local_rpc_tables();
    bounded_handoff_transitions();
    resumable_stream_framing()?;
    fallible_fixed_construction()?;
    bounded_lists_and_groups()?;
    recursive_fixed_copies()?;
    bounded_struct_upgrades()?;
    bounded_list_upgrades()?;
    bounded_list_promotions()?;
    bounded_generic_fields()?;
    bounded_capability_references();
    bounded_import_ownership();
    bounded_export_ownership();
    leased_message_roundtrip()?;
    suspended_request_and_nested_call()?;
    suspended_result_and_nested_output()?;
    bounded_rpc_publication()?;
    generated_rpc_conversation(3, false)?;
    generated_rpc_conversation(1, true)?;
    bounded_promise_resolution(1, false)?;
    bounded_promise_resolution(3, true)?;
    bounded_loopback_ordering()?;
    bounded_imported_clients()?;
    bounded_dispatch_ordering(1)?;
    generated_async_server()?;
    owned_rpc_conversation(3, false)?;
    owned_rpc_conversation(1, true)?;
    owned_task_failures()?;
    graph_connection();
    owning_connection_driver(3, false);
    owning_connection_driver(1, true);
    owning_return_resolve_order();
    owning_loopback_driver(3, true);
    owning_loopback_unresolved(3);
    owning_terminal_cleanup(1);
    owning_tail_driver(3);
    owning_tail_pipeline();
    owning_invocation_driver(3);
    owning_invocation_ordering();
    for mode in 0..7 {
        tracked_reply_delivery(mode);
    }
    for mode in 0..4 {
        local_reply_ownership(mode);
    }
    for mode in 0..6 {
        queued_local_reply_bindings(mode);
    }
    for mode in 0..6 {
        queued_remote_reply_bindings(mode);
    }
    generated_context_nested_call();
    for abandon in [false, true] {
        caller_polled_session(core::task::Waker::noop(), abandon, |_| ());
    }
    typed_capability_calls(false);
    typed_capability_calls(true);
    for mode in 0..7 {
        streaming_acknowledgment_pump(mode);
        streaming_session_calls(mode);
    }
    for mode in 0..8 {
        typed_session_calls(mode);
    }
    for mode in 0..9 {
        owned_result_draft(mode);
    }
    generated_context_wire_dispatch()?;
    caller_owned_reply_future(0);
    caller_owned_reply_future(1);
    caller_owned_reply_future(2);
    caller_owned_reply_future(3);
    queued_invocation_pipeline(false, true);
    local_invocation_pipeline(false);
    local_invocation_pipeline(true);
    owning_protocol_controls(false);
    owning_protocol_controls(true);
    owning_reflected_output(false);
    owning_reflected_output(true);
    exported_promise_failure();
    graph_resolution_retry();
    outgoing_question_pipeline(false, false);
    outgoing_question_pipeline(true, true);
    generated_branded_server()?;
    bounded_reflected_capabilities()?;
    bounded_reflected_return()?;
    bounded_tail_adoption(1, false)?;
    bounded_tail_adoption(3, true)?;
    bounded_reflected_resolution(1, false, false)?;
    bounded_reflected_resolution(3, true, true)?;
    bounded_wire_capabilities()?;
    bounded_pipeline_and_streaming()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn suspended_result_and_nested_output() {
        super::suspended_result_and_nested_output().unwrap();
    }
    #[test]
    fn bounded_tail_adoption() {
        for chunk in [1, 3, 7, 4096] {
            for canceled in [false, true] {
                super::bounded_tail_adoption(chunk, canceled).unwrap();
            }
        }
    }

    #[test]
    fn bounded_reflected_resolution() {
        for chunk in [1, 3, 7, 4096] {
            for promised in [false, true] {
                for abandoned in [false, true] {
                    super::bounded_reflected_resolution(chunk, promised, abandoned).unwrap();
                }
            }
        }
    }

    #[test]
    fn bounded_reflected_return() {
        super::bounded_reflected_return().unwrap();
    }
    #[test]
    fn bounded_reflected_capabilities() {
        super::bounded_reflected_capabilities().unwrap();
    }
    #[test]
    fn bounded_dispatch_ordering() {
        for chunk in [1, 7, 1024] {
            super::bounded_dispatch_ordering(chunk).unwrap();
        }
    }
    #[test]
    fn generated_async_server() {
        super::generated_async_server().unwrap();
    }

    #[test]
    fn owned_rpc_conversation() {
        for chunk in [1, 7, 1024] {
            for cancel in [false, true] {
                super::owned_rpc_conversation(chunk, cancel).unwrap();
            }
        }
    }

    #[test]
    fn owning_connection_driver() {
        for chunk in [1, 7, 1024] {
            for cancel in [false, true] {
                super::owning_connection_driver(chunk, cancel);
            }
        }
    }
    #[test]
    fn owning_reflected_output() {
        super::owning_reflected_output(false);
        super::owning_reflected_output(true);
    }
    #[test]
    fn exported_promise_failure() {
        super::exported_promise_failure();
    }
    #[test]
    fn owning_loopback_driver() {
        for chunk in [1, 7, 1024] {
            for reject in [false, true] {
                super::owning_loopback_driver(chunk, reject);
            }
        }
    }
    #[test]
    fn owning_loopback_unresolved() {
        for chunk in [1, 7, 1024] {
            super::owning_loopback_unresolved(chunk);
        }
    }
    #[test]
    fn owning_protocol_controls() {
        super::owning_protocol_controls(false);
        super::owning_protocol_controls(true);
    }
    #[test]
    fn owning_tail_pipeline() {
        super::owning_tail_pipeline();
    }
    #[test]
    fn owning_invocation_driver() {
        for chunk in [1, 7, 1024] {
            super::owning_invocation_driver(chunk);
        }
    }
    #[test]
    fn encoded_immediate_return() {
        for chunk in [1, 7, 1024] {
            super::driver_invocation::encoded_immediate_return(chunk);
        }
    }
    #[test]
    fn immediate_publication() {
        for chunk in [1, 7, 1024] {
            super::driver_invocation::immediate_publication(chunk);
        }
    }
    #[test]
    fn caller_owned_reply_future() {
        for mode in 0..4 {
            super::caller_owned_reply_future(mode);
        }
    }
    #[test]
    fn owning_invocation_ordering() {
        super::owning_invocation_ordering();
    }
    #[test]
    fn queued_invocation_pipeline() {
        for early in [false, true] {
            for cancel in [false, true] {
                super::queued_invocation_pipeline(early, cancel);
            }
        }
    }
    #[test]
    fn local_invocation_pipeline() {
        super::local_invocation_pipeline(false);
        super::local_invocation_pipeline(true);
    }
    #[test]
    fn generated_context_wire_dispatch() {
        super::generated_context_wire_dispatch().unwrap();
    }
    #[test]
    fn generated_context_nested_call() {
        super::generated_context_nested_call();
    }
    #[test]
    fn owned_result_draft() {
        for mode in 0..9 {
            super::owned_result_draft(mode);
        }
    }
    #[test]
    fn typed_session_calls() {
        for mode in 0..8 {
            super::typed_session_calls(mode);
        }
    }
    #[test]
    fn streaming_acknowledgment_pump() {
        for mode in 0..7 {
            super::streaming_acknowledgment_pump(mode);
        }
    }
    #[cfg(feature = "alloc")]
    #[test]
    fn growing_streaming_acknowledgment_pump() {
        for mode in 0..7 {
            super::growing_streaming_acknowledgment_pump(mode);
        }
    }

    #[cfg(feature = "alloc")]
    #[test]
    fn growing_ledger_streaming_acknowledgment_pump() {
        for mode in 0..7 {
            super::streaming_pump::growing_ledger_streaming_acknowledgment_pump(mode);
        }
    }

    #[cfg(feature = "alloc")]
    #[test]
    fn growing_ledger_cleanup() {
        super::streaming_pump::growing_ledger_cleanup();
    }
    #[cfg(feature = "growing")]
    #[test]
    fn growing_binding_cleanup() {
        super::streaming_pump::growing_binding_cleanup();
    }
    #[test]
    fn streaming_session_calls() {
        for mode in 0..7 {
            super::streaming_session_calls(mode);
        }
    }
    #[test]
    fn typed_capability_calls() {
        super::typed_capability_calls(false);
        super::typed_capability_calls(true);
    }
    #[test]
    fn queued_remote_reply_bindings() {
        for mode in 0..6 {
            super::queued_remote_reply_bindings(mode);
        }
    }
    #[test]
    fn queued_local_reply_bindings() {
        for mode in 0..6 {
            super::queued_local_reply_bindings(mode);
        }
    }
    #[test]
    fn local_reply_ownership() {
        for mode in 0..4 {
            super::local_reply_ownership(mode);
        }
    }
    #[test]
    fn tracked_reply_delivery() {
        for mode in 0..7 {
            super::tracked_reply_delivery(mode);
        }
    }
    // This assertion remains active for both core-only and alloc-enabled fixed
    // providers. The separate growing profile deliberately allocates storage.
    #[cfg(not(any(miri, feature = "growing")))]
    #[test]
    fn owning_driver_paths_do_not_allocate() {
        let count = allocation_counter::measure(|| {
            for abandon in [false, true] {
                super::caller_polled_session(core::task::Waker::noop(), abandon, |_| ());
            }
            for mode in 0..6 {
                super::owning_network_vine(mode);
            }
            super::owning_network_rendezvous(false);
            super::owning_network_rendezvous(true);
            for mode in 0..16 {
                super::owning_network_await(mode);
            }
            for mode in 0..6 {
                super::owning_network_join(mode);
            }
            for mode in 0..5 {
                super::owning_network_barrier(mode);
            }
            for mode in 0..3 {
                super::owning_adoption_sender(mode);
            }
            for mode in 0..3 {
                super::owning_adopted_capability(mode);
            }
            for mode in 0..8 {
                super::owning_network_adoption(mode);
            }
            super::owning_provision_sender();
            super::bounded_provision_lifecycle();
            super::bounded_provision_exchange(false);
            super::bounded_provision_exchange(true);
            for mode in 0..5 {
                super::owning_network_registration(mode);
            }
            for mode in 0..7 {
                super::owning_network_acceptance(mode);
            }
            for mode in 0..7 {
                super::tracked_reply_delivery(mode);
            }
            for mode in 0..4 {
                super::local_reply_ownership(mode);
            }
            for mode in 0..6 {
                super::queued_local_reply_bindings(mode);
            }
            for mode in 0..6 {
                super::queued_remote_reply_bindings(mode);
            }
            super::generated_context_nested_call();
            super::typed_capability_calls(false);
            super::typed_capability_calls(true);
            for mode in 0..7 {
                super::streaming_acknowledgment_pump(mode);
                super::streaming_session_calls(mode);
            }
            for mode in 0..8 {
                super::typed_session_calls(mode);
            }
            for mode in 0..9 {
                super::owned_result_draft(mode);
            }
            super::generated_context_wire_dispatch().unwrap();
            super::owning_invocation_driver(1);
            super::owning_invocation_ordering();
            for mode in 0..4 {
                super::caller_owned_reply_future(mode);
            }
            super::queued_invocation_pipeline(false, true);
            super::local_invocation_pipeline(false);
            super::local_invocation_pipeline(true);
            super::owning_tail_driver(1);
            super::owning_tail_pipeline();
            super::owning_loopback_driver(1, true);
            super::owning_loopback_unresolved(1);
            super::owning_terminal_cleanup(1);
            super::owning_protocol_controls(false);
        });
        assert_eq!(count.count_total, 0);
    }
    #[test]
    fn bounded_provision_lifecycle() {
        super::bounded_provision_lifecycle();
    }
    #[test]
    fn bounded_provision_exchange() {
        super::bounded_provision_exchange(false);
        super::bounded_provision_exchange(true);
    }
    #[test]
    fn owning_network_registration() {
        for mode in 0..5 {
            super::owning_network_registration(mode);
        }
    }
    #[test]
    fn owning_provision_sender() {
        super::owning_provision_sender();
    }
    #[test]
    fn owning_network_acceptance() {
        for mode in 0..7 {
            super::owning_network_acceptance(mode);
        }
    }
    #[test]
    fn owning_network_await() {
        for mode in 0..16 {
            super::owning_network_await(mode);
        }
    }
    #[test]
    fn owning_network_rendezvous() {
        super::owning_network_rendezvous(false);
        super::owning_network_rendezvous(true);
    }
    #[test]
    fn owning_network_join() {
        for mode in 0..6 {
            super::owning_network_join(mode);
        }
    }
    #[test]
    fn owning_network_vine() {
        for mode in 0..6 {
            super::owning_network_vine(mode);
        }
    }
    #[test]
    fn owning_network_barrier() {
        for mode in 0..5 {
            super::owning_network_barrier(mode);
        }
    }
    #[test]
    fn owning_adoption_sender() {
        for mode in 0..3 {
            super::owning_adoption_sender(mode);
        }
    }
    #[test]
    fn owning_adopted_capability() {
        for mode in 0..3 {
            super::owning_adopted_capability(mode);
        }
    }
    #[test]
    fn owning_network_adoption() {
        for mode in 0..8 {
            super::owning_network_adoption(mode);
        }
    }
    #[test]
    fn owning_tail_driver() {
        for chunk in [1, 7, 1024] {
            super::owning_tail_driver(chunk);
        }
    }
    #[test]
    fn owning_terminal_cleanup() {
        for budget in [1, 7, 128] {
            super::owning_terminal_cleanup(budget);
        }
    }
    #[test]
    fn owning_return_resolve_order() {
        super::owning_return_resolve_order();
    }
    #[test]
    fn owned_task_failures() {
        super::owned_task_failures().unwrap();
    }
    #[test]
    fn graph_connection() {
        super::graph_connection();
    }
    #[test]
    fn graph_resolution_retry() {
        super::graph_resolution_retry();
    }
    #[test]
    fn outgoing_question_pipeline() {
        for reflected in [false, true] {
            for cancel in [false, true] {
                super::outgoing_question_pipeline(reflected, cancel);
            }
        }
    }
    #[test]
    fn generated_branded_server() {
        super::generated_branded_server().unwrap();
    }
    #[test]
    fn bounded_imported_clients() {
        super::bounded_imported_clients().unwrap();
    }
    #[test]
    fn bounded_loopback_ordering() {
        super::bounded_loopback_ordering().unwrap();
    }
    #[test]
    fn bounded_promise_resolution() {
        for chunk in [1, 3, 7, 4096] {
            for abandoned in [false, true] {
                super::bounded_promise_resolution(chunk, abandoned).unwrap();
            }
        }
    }
    #[test]
    fn suspended_request_and_nested_call() {
        super::suspended_request_and_nested_call().unwrap();
    }
    #[test]
    fn bounded_pipeline_and_streaming() {
        super::bounded_pipeline_and_streaming().unwrap();
    }
    #[test]
    fn bounded_wire_capabilities() {
        super::bounded_wire_capabilities().unwrap();
    }
    #[test]
    fn generated_rpc_conversation() {
        for chunk in [1, 3, 7, 8, 13, 4096] {
            for cancel in [false, true] {
                super::generated_rpc_conversation(chunk, cancel).unwrap();
            }
        }
    }
    #[test]
    fn bounded_server_output() {
        super::bounded_server_output();
    }

    #[test]
    fn bounded_resource_output() {
        super::bounded_resource_output();
    }
    #[test]
    fn bounded_rpc_publication() {
        super::bounded_rpc_publication().unwrap();
    }
    #[test]
    fn leased_message_roundtrip() {
        super::leased_message_roundtrip().unwrap();
    }
    #[test]
    fn bounded_export_ownership() {
        super::bounded_export_ownership();
    }
    #[test]
    fn bounded_import_ownership() {
        super::bounded_import_ownership();
    }
    #[test]
    fn bounded_capability_references() {
        super::bounded_capability_references();
    }
    #[test]
    fn bounded_list_upgrades() {
        super::bounded_list_upgrades().unwrap();
    }
    #[test]
    fn bounded_list_promotions() {
        super::bounded_list_promotions().unwrap();
    }
    #[test]
    fn bounded_generic_fields() {
        super::bounded_generic_fields().unwrap();
    }
    #[test]
    fn bounded_struct_upgrades() {
        super::bounded_struct_upgrades().unwrap();
    }
    #[test]
    fn bounded_lists_and_groups() {
        super::bounded_lists_and_groups().unwrap();
    }
    #[test]
    fn recursive_fixed_copies() {
        super::recursive_fixed_copies().unwrap();
    }
    #[test]
    fn fixed_buffer_views_and_edits() {
        super::fixed_buffer_views_and_edits().unwrap();
    }
    #[test]
    fn defaults_and_union_rejection() {
        super::defaults_and_union_rejection().unwrap();
    }
    #[test]
    fn malformed_borrowed_field() {
        super::malformed_borrowed_field().unwrap();
    }
    #[test]
    fn invalid_utf8_preserves_wire_access() {
        super::invalid_utf8_preserves_wire_access().unwrap();
    }
    #[test]
    fn caller_supplied_diagnostics() {
        super::caller_supplied_diagnostics();
    }
    #[test]
    fn bounded_rpc_tables() {
        super::bounded_rpc_tables();
    }
    #[test]
    fn bounded_local_rpc_tables() {
        super::bounded_local_rpc_tables();
    }
    #[test]
    fn bounded_handoff_transitions() {
        super::bounded_handoff_transitions();
    }
    #[test]
    fn resumable_stream_framing() {
        super::resumable_stream_framing().unwrap();
    }
    #[test]
    fn fallible_fixed_construction() {
        super::fallible_fixed_construction().unwrap();
    }
}
