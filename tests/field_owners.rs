use capnp::field_api::{CapabilityTable, Message, MessageReader, MessageView};
use capnp::message::{HeapAllocator, Reader, ReaderOptions, ReaderSegments};
use capnp::{ErrorKind, Result, Word};
use capntproto_test_support::field_api_capnp::{
    api::{Address, AddressRef, NativeRecord},
    service,
};
use std::{
    borrow::{Borrow, BorrowMut},
    cell::Cell,
    rc::Rc,
};

#[derive(Default, Clone)]
struct Counts {
    segments: Rc<Cell<usize>>,
    context: Rc<Cell<usize>>,
    server: Rc<Cell<usize>>,
}
struct Context {
    table: CapabilityTable,
    counts: Counts,
}
impl Borrow<CapabilityTable> for Context {
    fn borrow(&self) -> &CapabilityTable {
        &self.table
    }
}
impl BorrowMut<CapabilityTable> for Context {
    fn borrow_mut(&mut self) -> &mut CapabilityTable {
        &mut self.table
    }
}
impl Drop for Context {
    fn drop(&mut self) {
        self.counts.context.set(self.counts.context.get() + 1);
    }
}
struct Segments {
    parts: Vec<Vec<Word>>,
    counts: Counts,
}
impl ReaderSegments for Segments {
    fn get_segment(&self, i: u32) -> Option<&[u8]> {
        self.parts.get(i as usize).map(|v| Word::words_to_bytes(v))
    }
    fn len(&self) -> usize {
        self.parts.len()
    }
}
impl Drop for Segments {
    fn drop(&mut self) {
        self.counts.segments.set(self.counts.segments.get() + 1);
    }
}
fn segments(input: &impl ReaderSegments, counts: &Counts) -> Segments {
    Segments {
        parts: (0..input.len())
            .map(|i| {
                let bytes = input.get_segment(i as u32).unwrap();
                let mut words = Word::allocate_zeroed_vec(bytes.len() / 8);
                Word::words_to_bytes_mut(&mut words).copy_from_slice(bytes);
                words
            })
            .collect(),
        counts: counts.clone(),
    }
}
struct Server(Counts);
impl service::Server for Server {
    async fn echo(
        self: Rc<Self>,
        params: service::EchoParams,
        mut results: service::EchoResults,
    ) -> Result<()> {
        let id = params.get()?.get_person()?.get_id();
        results.get().init_person().set_id(id + 1);
        Ok(())
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.0.server.set(self.0.server.get() + 1);
    }
}
fn message(
    counts: &Counts,
    far: bool,
) -> Result<Message<NativeRecord, capnp::field_api::Mutable, HeapAllocator, Context>> {
    message_with_executor(counts, far, None)
}
fn message_with_executor(
    counts: &Counts,
    far: bool,
    executor: Option<Rc<dyn capnp::capability::CallExecutor>>,
) -> Result<Message<NativeRecord, capnp::field_api::Mutable, HeapAllocator, Context>> {
    let mut allocator = HeapAllocator::new();
    if far {
        allocator = allocator
            .first_segment_words(1)
            .allocation_strategy(capnp::message::AllocationStrategy::FixedSize);
    }
    let mut message = Message::<NativeRecord, _, _, _>::with_allocator_and_capabilities(
        allocator,
        Context {
            table: Vec::new(),
            counts: counts.clone(),
        },
    )?;
    let cap: service::Client = match executor {
        Some(e) => capnp_rpc::new_client_with_executor(Server(counts.clone()), e),
        None => capnp_rpc::new_client(Server(counts.clone())),
    };
    message.edit().cap().copy_from(cap)?;
    message.edit().label().copy_from("x")?;
    Ok(message)
}
async fn call(cap: &service::Client) -> Result<()> {
    let mut request = cap.echo_request();
    request.get().init_person().set_id(41);
    assert_eq!(
        request.send().promise.await?.get()?.get_person()?.get_id(),
        42
    );
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn custom_segment_and_capability_owners_preserve_authority_and_independent_copies(
) -> Result<()> {
    tokio::task::LocalSet::new()
        .run_until(async {
            for far in [false, true] {
                let counts = Counts::default();
                let (executor, driver) = capnp_rpc::new_call_executor();
                let driver = tokio::task::spawn_local(driver);
                let (storage, context) =
                    message_with_executor(&counts, far, Some(executor))?.into_parts();
                let parts = segments(&storage, &counts);
                assert_eq!(parts.len() > 1, far);
                drop(storage);
                let reader = MessageReader::<NativeRecord, _, _>::from_segments_with_capabilities(
                    parts,
                    ReaderOptions::new(),
                    context,
                )?;
                let address = reader.read().label()?.as_ptr();
                let cap = reader.read().cap()?;
                let identity = cap.client.hook.get_ptr();
                // Move both the table owner and outer wrapper; the shared allocation stays put.
                let mut owners = vec![Box::new(reader)];
                let owner = owners.pop().unwrap();
                assert_eq!(owner.read().label()?.as_ptr(), address);
                assert_eq!(owner.read().cap()?.client.hook.get_ptr(), identity);
                let copy = owner.compact_copy()?;
                assert_ne!(copy.read().label()?.as_ptr(), address);
                assert_eq!(copy.read().cap()?.client.hook.get_ptr(), identity);
                let (reader, context) = owner.into_parts();
                assert_eq!(counts.segments.get(), 0);
                let owner = MessageReader::<NativeRecord, _, _>::from_reader_with_capabilities(
                    reader, context,
                )?;
                assert_eq!(owner.read().label()?.as_ptr(), address);
                drop(owner);
                assert_eq!(counts.segments.get(), 1);
                assert_eq!(counts.context.get(), 1);
                assert_eq!(counts.server.get(), 0);
                call(&cap).await?;
                drop(cap);
                call(&copy.read().cap()?).await?;
                drop(copy);
                assert_eq!(counts.server.get(), 1);
                driver.abort();
            }
            Ok(())
        })
        .await
}

#[test]
fn inline_segment_providers_are_pinned_before_root_acquisition() -> Result<()> {
    struct Inline([Word; 4]);
    impl ReaderSegments for Inline {
        fn get_segment(&self, i: u32) -> Option<&[u8]> {
            (i == 0).then(|| Word::words_to_bytes(&self.0))
        }
    }
    let mut input = Message::<Address>::new()?;
    input.edit().city().copy_from("small")?;
    let (storage, _) = input.into_parts();
    let bytes = storage.get_segment(0).unwrap();
    assert_eq!(bytes.len(), 32);
    let mut inline = Inline([capnp::word(0, 0, 0, 0, 0, 0, 0, 0); 4]);
    Word::words_to_bytes_mut(&mut inline.0).copy_from_slice(bytes);
    let reader = MessageReader::<Address, _>::from_segments(inline, ReaderOptions::new())?;
    let pointer = reader.read().city()?.as_ptr();
    let reader = Box::new(reader);
    assert_eq!(reader.read().city()?.as_ptr(), pointer);
    let (raw, table) = reader.into_parts();
    let reader = MessageReader::<Address, _, _>::from_reader_with_capabilities(raw, table)?;
    assert_eq!(reader.read().city()?, "small");
    Ok(())
}

#[test]
fn borrowed_and_shared_capability_tables_are_explicit_and_bytes_grant_no_authority() -> Result<()> {
    let counts = Counts::default();
    let (storage, context) = message(&counts, false)?.freeze().into_parts();
    let bytes = capnp::serialize::write_message_to_words(&storage);
    let empty = MessageView::<NativeRecord>::from_unpacked(&bytes, ReaderOptions::new())?;
    assert!(empty.read().cap().is_err());
    let identity = context.table[0].as_ref().unwrap().get_ptr();
    let shared = Rc::new(context);
    // A custom wrapper supports arbitrary immutable context ownership.
    struct Shared(Rc<Context>);
    impl Borrow<CapabilityTable> for Shared {
        fn borrow(&self) -> &CapabilityTable {
            &self.0.table
        }
    }
    let view = MessageView::<NativeRecord, _>::from_unpacked_with_capabilities(
        &bytes,
        ReaderOptions::new(),
        Shared(shared.clone()),
    )?;
    let held = view.read().cap()?;
    assert_eq!(held.client.hook.get_ptr(), identity);
    let mut frames = bytes.clone();
    frames.extend_from_slice(&bytes);
    assert_eq!(
        MessageView::<NativeRecord, _>::from_unpacked_with_capabilities(
            &frames,
            ReaderOptions::new(),
            &shared.table
        )
        .err()
        .unwrap()
        .kind,
        ErrorKind::TrailingData
    );
    let (prefix, rest) = MessageView::<NativeRecord, _>::from_unpacked_prefix_with_capabilities(
        &frames,
        ReaderOptions::new(),
        &shared.table,
    )?;
    assert_eq!(rest, bytes);
    assert_eq!(prefix.read().cap()?.client.hook.get_ptr(), identity);
    drop(prefix);
    drop(shared);
    drop(view);
    assert_eq!(counts.context.get(), 1);
    assert_eq!(counts.server.get(), 0);
    drop(held);
    assert_eq!(counts.server.get(), 1);
    Ok(())
}

#[test]
fn freezing_and_reading_retain_exclusively_borrowed_contexts_and_scratch_storage() -> Result<()> {
    let counts = Counts::default();
    let mut table = CapabilityTable::new();
    let mut scratch = Word::allocate_zeroed_vec(128);
    let allocator =
        capnp::message::ScratchSpaceHeapAllocator::new(Word::words_to_bytes_mut(&mut scratch));
    let mut owner =
        Message::<NativeRecord, _, _, _>::with_allocator_and_capabilities(allocator, &mut table)?;
    owner
        .edit()
        .cap()
        .copy_from(capnp_rpc::new_client(Server(counts.clone())))?;
    owner.edit().label().copy_from("scratch")?;
    let pointer = owner.read().label()?.as_ptr();
    let frozen = owner.freeze();
    assert_eq!(frozen.read().label()?.as_ptr(), pointer);
    let reader = frozen.into_reader(ReaderOptions::new())?;
    assert_eq!(reader.read().label()?.as_ptr(), pointer);
    let held = reader.read().cap()?;
    drop(reader);
    assert_eq!(counts.server.get(), 0);
    table.clear();
    assert_eq!(counts.server.get(), 0);
    drop(held);
    assert_eq!(counts.server.get(), 1);
    Ok(())
}

#[test]
fn existing_reader_limits_survive_transfers_and_failed_copy_releases_partial_hooks() -> Result<()> {
    let counts = Counts::default();
    let (storage, context) = message(&counts, false)?.into_parts();
    let limits = ReaderOptions {
        traversal_limit_in_words: Some(5),
        ..ReaderOptions::new()
    };
    let reader = MessageReader::<NativeRecord, _, _>::from_segments_with_capabilities(
        storage, limits, context,
    )?;
    // Root pointer and struct acquisition cost three words. Each label read costs one.
    assert_eq!(reader.read().label()?, "x");
    assert_eq!(reader.read().label()?, "x");
    assert_eq!(
        reader.read().label().err().unwrap().kind,
        ErrorKind::ReadLimitExceeded
    );
    let held = reader.read().cap()?;
    assert!(reader.compact_copy().is_err());
    let (reader, context) = reader.into_parts();
    assert!(
        MessageReader::<NativeRecord, _, _>::from_reader_with_capabilities(reader, context)
            .is_err()
    );
    assert_eq!(counts.context.get(), 1);
    assert_eq!(counts.server.get(), 0);
    drop(held);
    assert_eq!(counts.server.get(), 1);
    // Prior consumption on a legacy reader must also carry across adoption.
    let mut source = Message::<Address>::new()?;
    source.edit().city().copy_from("x")?;
    let (source, _) = source.into_parts();
    let raw = Reader::new(
        source,
        ReaderOptions {
            traversal_limit_in_words: Some(2),
            ..ReaderOptions::new()
        },
    );
    assert_eq!(
        raw.get_root::<AddressRef<'_>>()?.city().err().unwrap().kind,
        ErrorKind::ReadLimitExceeded
    );
    assert!(MessageReader::<Address, _>::from_reader(raw).is_err());
    Ok(())
}

#[test]
fn malformed_acquisition_releases_owners_and_descendants_remain_lazy() -> Result<()> {
    let counts = Counts::default();
    let (storage, context) = message(&counts, false)?.into_parts();
    let mut parts = segments(&storage, &counts);
    drop(storage);
    // Invalid root pointer, rejected before publishing the owner.
    Word::words_to_bytes_mut(&mut parts.parts[0])[..8].copy_from_slice(&u64::MAX.to_le_bytes());
    assert!(
        MessageReader::<NativeRecord, _, _>::from_segments_with_capabilities(
            parts,
            ReaderOptions::new(),
            context
        )
        .is_err()
    );
    assert_eq!(counts.segments.get(), 1);
    assert_eq!(counts.context.get(), 1);
    assert_eq!(counts.server.get(), 1);
    let mut m = Message::<Address>::new()?;
    m.edit().city().copy_from("lazy")?;
    let (storage, _) = m.into_parts();
    let mut parts = segments(&storage, &Counts::default());
    Word::words_to_bytes_mut(&mut parts.parts[0])[8..16].copy_from_slice(&u64::MAX.to_le_bytes());
    let view = MessageReader::<Address, _>::from_segments(parts, ReaderOptions::new())?;
    assert!(view.read().city().is_err());
    Ok(())
}

// The allocator and provider are real storage owners, not model-side lifetime flags.
struct TrackedAllocator {
    inner: HeapAllocator,
    counts: Counts,
}
// SAFETY: allocation/deallocation and every size/alignment promise are delegated
// to HeapAllocator. The extra field only observes destruction of the owner.
unsafe impl capnp::message::Allocator for TrackedAllocator {
    fn allocate_segment(&mut self, minimum_size: u32) -> (*mut u8, u32) {
        self.inner.allocate_segment(minimum_size)
    }
    unsafe fn deallocate_segment(&mut self, ptr: *mut u8, word_size: u32, words_used: u32) {
        unsafe { self.inner.deallocate_segment(ptr, word_size, words_used) }
    }
}
impl Drop for TrackedAllocator {
    fn drop(&mut self) {
        self.counts.segments.set(self.counts.segments.get() + 1);
    }
}
struct Provider(capnp::message::Builder<TrackedAllocator>);
impl ReaderSegments for Provider {
    fn get_segment(&self, i: u32) -> Option<&[u8]> {
        self.0.get_segment(i)
    }
    fn len(&self) -> usize {
        self.0.len()
    }
}
type MutableOwner = Message<NativeRecord, capnp::field_api::Mutable, TrackedAllocator, Context>;
type FrozenOwner = capnp::field_api::FrozenMessage<NativeRecord, TrackedAllocator, Context>;
type ReadOwner = MessageReader<NativeRecord, Provider, Context>;
enum Source {
    Mutable(MutableOwner),
    Frozen(FrozenOwner),
    Reader(ReadOwner),
    Parts(Reader<Provider>, Context),
}
impl Source {
    fn phase(&self) -> usize {
        match self {
            Self::Mutable(_) => 1,
            Self::Frozen(_) => 2,
            Self::Reader(_) => 3,
            Self::Parts(..) => 4,
        }
    }
    fn moved(self) -> Self {
        *Box::new(self)
    }
}
#[derive(Debug, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct TraceState {
    phase: usize,
    budget: usize,
    spent: usize,
    moved: usize,
    detached: usize,
    extracted: usize,
    held: usize,
    copy_tried: usize,
    copied: usize,
    copy_ok: usize,
    read_failed: usize,
    probed: usize,
    unauthorized: usize,
    identity: usize,
    storage_alive: usize,
    context_alive: usize,
    cap_alive: usize,
}
#[derive(serde::Deserialize)]
struct Trace {
    far: bool,
    steps: Vec<Step>,
}
#[derive(serde::Deserialize)]
struct Step {
    action: String,
    state: TraceState,
}
#[test]
fn replay_tlc_field_owner_traces() -> Result<()> {
    let path = capntproto_test_support::verification::input("CAPNTPROTO_FIELD_OWNER_TRACES")
        .expect("prepare verified trace corpus");
    let traces: Vec<Trace> = capntproto_test_support::traces::read(path, "RpcFieldOwners");
    assert!(!traces.is_empty());
    for trace in traces {
        let counts = Counts::default();
        let mut inner = HeapAllocator::new();
        if trace.far {
            inner = inner
                .first_segment_words(1)
                .allocation_strategy(capnp::message::AllocationStrategy::FixedSize);
        }
        let allocator = TrackedAllocator {
            inner,
            counts: counts.clone(),
        };
        let mut owner = MutableOwner::with_allocator_and_capabilities(
            allocator,
            Context {
                table: Vec::new(),
                counts: counts.clone(),
            },
        )?;
        let cap: service::Client = capnp_rpc::new_client(Server(counts.clone()));
        let identity = cap.client.hook.get_ptr();
        owner.edit().cap().copy_from(cap)?;
        owner.edit().label().copy_from("x")?;
        let bytes = owner.to_vec();
        let pointer = owner.read().label()?.as_ptr();
        let read_cost = if trace.far { 2 } else { 1 };
        let root_cost = if trace.far { 4 } else { 3 };
        let limits = ReaderOptions {
            traversal_limit_in_words: Some(root_cost + 3 * read_cost),
            ..ReaderOptions::new()
        };
        let mut source = Some(Source::Mutable(owner));
        let mut copied: Option<Message<NativeRecord>> = None;
        let mut held: Option<service::Client> = None;
        for (index, step) in trace.steps.iter().enumerate() {
            match step.action.as_str() {
                "freeze" => {
                    let Some(Source::Mutable(m)) = source.take() else {
                        panic!("mutable")
                    };
                    source = Some(Source::Frozen(m.freeze()));
                }
                "reader" => {
                    let (storage, context) = match source.take().unwrap() {
                        Source::Mutable(m) => m.into_parts(),
                        Source::Frozen(m) => m.into_parts(),
                        _ => panic!("builder"),
                    };
                    source = Some(Source::Reader(ReadOwner::from_segments_with_capabilities(
                        Provider(storage),
                        limits,
                        context,
                    )?));
                }
                "move" => source = source.take().map(Source::moved),
                "detach" => {
                    let Some(Source::Reader(r)) = source.take() else {
                        panic!("reader")
                    };
                    let (r, c) = r.into_parts();
                    source = Some(Source::Parts(r, c));
                }
                "reattach" => {
                    let Some(Source::Parts(r, c)) = source.take() else {
                        panic!("parts")
                    };
                    match ReadOwner::from_reader_with_capabilities(r, c) {
                        Ok(r) => {
                            assert_eq!(step.state.phase, 3, "reattach at {index}");
                            source = Some(Source::Reader(r));
                        }
                        Err(e) => {
                            assert_eq!(step.state.phase, 0, "reattach at {index}: {e}");
                            assert_eq!(e.kind, ErrorKind::ReadLimitExceeded);
                        }
                    }
                }
                "read" | "failRead" => {
                    let Some(Source::Reader(r)) = &source else {
                        panic!("reader")
                    };
                    let result = r.read().label();
                    if step.action == "read" {
                        let text = result.unwrap_or_else(|e| {
                            panic!("far={} step={index} state={:?}: {e}", trace.far, step.state)
                        });
                        assert_eq!(text, "x");
                        assert_eq!(text.as_ptr(), pointer);
                    } else {
                        assert_eq!(
                            result.expect_err("budget exhaustion").kind,
                            ErrorKind::ReadLimitExceeded
                        );
                    }
                }
                "extract" => {
                    held = Some(if let Some(Source::Reader(r)) = &source {
                        r.read().cap()?
                    } else {
                        copied.as_ref().unwrap().read().cap()?
                    });
                }
                "copy" => {
                    let Some(Source::Reader(r)) = &source else {
                        panic!("reader")
                    };
                    match r.compact_copy() {
                        Ok(c) => {
                            assert_eq!(step.state.copied, 1, "copy at {index}");
                            copied = Some(c);
                        }
                        Err(e) => {
                            assert_eq!(step.state.copied, 0, "copy at {index}: {e}");
                            assert_eq!(e.kind, ErrorKind::ReadLimitExceeded);
                        }
                    }
                }
                "dropSource" => {
                    source.take();
                }
                "dropHeld" => {
                    held.take();
                }
                "dropCopy" => {
                    copied.take();
                }
                "probe" => {
                    let r =
                        MessageView::<NativeRecord>::from_unpacked(&bytes, ReaderOptions::new())?;
                    assert!(r.read().cap().is_err());
                }
                _ => panic!("unknown action"),
            }
            assert_eq!(source.as_ref().map_or(0, Source::phase), step.state.phase);
            assert_eq!(usize::from(held.is_some()), step.state.held);
            assert_eq!(usize::from(copied.is_some()), step.state.copied);
            assert_eq!(
                usize::from(counts.segments.get() == 0),
                step.state.storage_alive
            );
            assert_eq!(
                usize::from(counts.context.get() == 0),
                step.state.context_alive
            );
            assert_eq!(usize::from(counts.server.get() == 0), step.state.cap_alive);
            assert!(
                counts.segments.get() <= 1 && counts.context.get() <= 1 && counts.server.get() <= 1
            );
            if let Some(Source::Reader(r)) = &source {
                assert_eq!(r.read().cap()?.client.hook.get_ptr(), identity);
            }
            if let Some(c) = &copied {
                assert_eq!(c.read().label()?, "x");
                assert_eq!(c.read().cap()?.client.hook.get_ptr(), identity);
            }
            if let Some(c) = &held {
                assert_eq!(c.client.hook.get_ptr(), identity);
            }
        }
        // Audit the remaining real traversal budget after EVERY exported edge,
        // including moves. This catches resets even when shortest model prefixes
        // put an equivalent move before the first reader acquisition.
        let budget = trace.steps.last().unwrap().state.budget;
        let reader = match source.take() {
            Some(Source::Reader(r)) => Some((r, budget)),
            Some(Source::Parts(r, c)) => match ReadOwner::from_reader_with_capabilities(r, c) {
                Ok(r) => {
                    assert!(budget >= root_cost);
                    Some((r, budget - root_cost))
                }
                Err(e) => {
                    assert!(budget < root_cost);
                    assert_eq!(e.kind, ErrorKind::ReadLimitExceeded);
                    None
                }
            },
            _ => None,
        };
        if let Some((reader, budget)) = reader {
            for _ in 0..budget / read_cost {
                assert_eq!(reader.read().label()?, "x");
            }
            assert_eq!(
                reader.read().label().expect_err("remaining budget").kind,
                ErrorKind::ReadLimitExceeded
            );
        }
        drop(copied);
        drop(held);
        assert_eq!(counts.segments.get(), 1);
        assert_eq!(counts.context.get(), 1);
        assert_eq!(counts.server.get(), 1);
    }
    Ok(())
}

#[test]
fn custom_context_orphans_reject_other_arenas_without_losing_authority() -> Result<()> {
    use capntproto_test_support::field_api_capnp::api::Transfer;
    let counts = Counts::default();
    let mut left = Message::<Transfer>::with_capabilities(Context {
        table: Vec::new(),
        counts: counts.clone(),
    })?;
    let mut right = Message::<Transfer>::with_capabilities(Context {
        table: Vec::new(),
        counts: counts.clone(),
    })?;
    left.edit()
        .source()
        .init()?
        .cap()
        .copy_from(capnp_rpc::new_client(Server(counts.clone())))?;
    let held = left.read().source()?.cap()?;
    let identity = held.client.hook.get_ptr();
    let (mut root, token) = left.edit_with_orphans();
    let orphan = root.source().take(&token)?.unwrap();
    let failure = right.edit().destination().adopt(orphan).unwrap_err();
    assert_eq!(failure.error.kind, ErrorKind::WrongArena);
    assert!(right.read().field(Transfer::DESTINATION).is_null());
    root.destination().adopt(failure.orphan).unwrap();
    assert_eq!(
        root.read().destination()?.cap()?.client.hook.get_ptr(),
        identity
    );
    drop(root.destination().take(&token)?);
    assert_eq!(counts.server.get(), 0);
    drop(left);
    drop(right);
    assert_eq!(counts.context.get(), 2);
    drop(held);
    assert_eq!(counts.server.get(), 1);
    Ok(())
}
