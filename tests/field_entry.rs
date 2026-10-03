use capnp::field_api::{self as api, Entry, Message, PointerType, Schema};
use capnp::message::{AllocationStrategy, HeapAllocator};
use capnp::private::{
    arena::{BuilderArena, BuilderArenaImpl, ReaderArena},
    layout::{CapTable, CapTableBuilder, ElementSize, PointerBuilder, StructBuilder, StructSize},
};
use capnp::{ErrorKind, Result};
use capntproto_test_support::field_api_capnp::{
    api::{NewRecord, Person, Types},
    new_record,
};
use std::{cell::Cell, rc::Rc};

#[derive(Default)]
struct Counts {
    accesses: Cell<usize>,
    allocations: Cell<usize>,
}
impl Counts {
    fn access(&self) {
        self.accesses.set(self.accesses.get() + 1);
    }
    fn allocation(&self) {
        self.allocations.set(self.allocations.get() + 1);
    }
}
struct Arena {
    inner: BuilderArenaImpl<HeapAllocator>,
    counts: Rc<Counts>,
}
// SAFETY: these implementations delegate all storage and lifetime guarantees
// to the production arena. Only independent counters are added; no pointers
// are retained outside their ordinary arena lifetime.
unsafe impl ReaderArena for Arena {
    fn get_segment(&self, id: u32) -> Result<(*const u8, u32)> {
        self.counts.access();
        self.inner.get_segment(id)
    }
    unsafe fn check_offset(&self, id: u32, start: *const u8, offset: i32) -> Result<*const u8> {
        self.counts.access();
        unsafe { self.inner.check_offset(id, start, offset) }
    }
    fn contains_interval(&self, id: u32, start: *const u8, size: usize) -> Result<()> {
        self.counts.access();
        self.inner.contains_interval(id, start, size)
    }
    fn amplified_read(&self, amount: u64) -> Result<()> {
        self.counts.access();
        self.inner.amplified_read(amount)
    }
    fn nesting_limit(&self) -> i32 {
        self.inner.nesting_limit()
    }
    fn size_in_words(&self) -> usize {
        self.inner.size_in_words()
    }
}
unsafe impl BuilderArena for Arena {
    fn allocate(&mut self, id: u32, amount: u32) -> Option<u32> {
        self.counts.allocation();
        self.inner.allocate(id, amount)
    }
    fn allocate_anywhere(&mut self, amount: u32) -> (u32, u32) {
        self.counts.allocation();
        self.inner.allocate_anywhere(amount)
    }
    fn get_segment_mut(&mut self, id: u32) -> (*mut u8, u32) {
        self.counts.access();
        self.inner.get_segment_mut(id)
    }
    fn as_reader(&self) -> &dyn ReaderArena {
        self
    }
    fn is_writable(&self, id: u32) -> bool {
        self.counts.access();
        self.inner.is_writable(id)
    }
    fn add_external_segment(&mut self, data: capnp::dynamic_orphan::ExternalData) -> Result<u32> {
        self.inner.add_external_segment(data)
    }
}
impl Arena {
    fn new(far: bool) -> Self {
        let allocator = if far {
            HeapAllocator::new()
                .first_segment_words(1)
                .allocation_strategy(AllocationStrategy::FixedSize)
        } else {
            HeapAllocator::new()
        };
        let mut inner = BuilderArenaImpl::new(allocator);
        assert_eq!(inner.allocate_anywhere(1), (0, 0));
        Self {
            inner,
            counts: Rc::new(Counts::default()),
        }
    }
    fn root(&mut self) -> PointerBuilder<'_> {
        let (pointer, _) = self.inner.get_segment_mut(0);
        PointerBuilder::get_root(self, 0, pointer)
    }
}
fn occupied<K: PointerType>(entry: Entry<'_, K>) -> api::OccupiedField<'_, K> {
    match entry {
        Entry::Occupied(v) => v,
        Entry::Vacant(_) => panic!("expected occupied"),
    }
}
fn check_cached<K: PointerType>(
    root: &mut StructBuilder<'_>,
    counts: &Counts,
    ensure: bool,
) -> Result<()> {
    let allocations = counts.allocations.get();
    let before = counts.accesses.get();
    let entry = occupied(api::PointerField::<K>::new(root.reborrow(), 0, None, None).entry()?);
    assert!(
        counts.accesses.get() > before,
        "inspection must actually acquire storage"
    );
    assert_eq!(counts.allocations.get(), allocations, "entry allocated");
    let acquired = counts.accesses.get();
    let editor = if ensure {
        entry.ensure()?
    } else {
        entry.edit()?
    };
    assert_eq!(
        counts.accesses.get(),
        acquired,
        "consumption reacquired storage"
    );
    assert_eq!(
        counts.allocations.get(),
        allocations,
        "consumption allocated"
    );
    drop(editor);
    Ok(())
}
#[test]
fn occupied_editors_reuse_acquisition_for_native_and_generic_pointer_kinds() -> Result<()> {
    for far in [false, true] {
        for ensure in [false, true] {
            let mut arena = Arena::new(far);
            let counts = arena.counts.clone();
            let mut root = arena.root().init_struct(StructSize {
                data: 0,
                pointers: 1,
            });
            root.reborrow()
                .get_pointer_field(0)
                .init_text(4)
                .push_str("text");
            check_cached::<api::Text>(&mut root, &counts, ensure)?;
            check_cached::<api::Generic<capnp::text::Owned>>(&mut root, &counts, ensure)?;
            root.reborrow()
                .get_pointer_field(0)
                .init_data(4)
                .copy_from_slice(b"data");
            check_cached::<api::Data>(&mut root, &counts, ensure)?;
            check_cached::<api::Generic<capnp::data::Owned>>(&mut root, &counts, ensure)?;
            root.reborrow()
                .get_pointer_field(0)
                .init_struct(NewRecord::SIZE)
                .set_data_field::<u64>(0, 7);
            check_cached::<api::Struct<NewRecord>>(&mut root, &counts, ensure)?;
            check_cached::<api::Generic<new_record::Owned>>(&mut root, &counts, ensure)?;
            check_cached::<api::Generic<capnp::any_pointer::Owned>>(&mut root, &counts, ensure)?;
            root.reborrow()
                .get_pointer_field(0)
                .init_list(ElementSize::FourBytes, 3);
            check_cached::<api::List<u32>>(&mut root, &counts, ensure)?;
            check_cached::<api::Generic<capnp::primitive_list::Owned<u32>>>(
                &mut root, &counts, ensure,
            )?;
            root.reborrow()
                .get_pointer_field(0)
                .init_struct_list(2, NewRecord::SIZE);
            check_cached::<api::List<NewRecord>>(&mut root, &counts, ensure)?;
            check_cached::<api::Generic<capnp::struct_list::Owned<new_record::Owned>>>(
                &mut root, &counts, ensure,
            )?;
            root.reborrow()
                .get_pointer_field(0)
                .init_list(ElementSize::Pointer, 2);
            check_cached::<api::List<api::Text>>(&mut root, &counts, ensure)?;
            check_cached::<api::List<api::List<u32>>>(&mut root, &counts, ensure)?;
            root.reborrow()
                .get_pointer_field(0)
                .init_list(ElementSize::Bit, 5);
            check_cached::<api::List<bool>>(&mut root, &counts, ensure)?;
            root.reborrow()
                .get_pointer_field(0)
                .init_list(ElementSize::Void, 5);
            check_cached::<api::List<()>>(&mut root, &counts, ensure)?;
        }
    }
    Ok(())
}
#[test]
fn entries_validate_text_preserve_defaults_and_do_not_select_inactive_union_arms() -> Result<()> {
    let mut message = Message::<Person>::new()?;
    assert!(matches!(message.edit().name().entry()?, Entry::Vacant(_)));
    assert_eq!(message.read().name()?, "anonymous");
    message.edit().name().copy_from("valid")?;
    message.edit().name().edit()?.as_bytes_mut()[0] = 255;
    let before = message.to_vec();
    assert!(message.edit().name().entry().is_err());
    assert_eq!(message.to_vec(), before);
    assert_eq!(
        message
            .edit()
            .employment()
            .school()
            .entry()
            .err()
            .unwrap()
            .kind,
        ErrorKind::NotPresent
    );
    assert_eq!(message.to_vec(), before);
    message
        .edit()
        .names()
        .init_with(1, |_, slot| slot.copy_from("list"))?;
    let mut person = message.edit();
    let mut names = person.names().edit()?;
    occupied(names.get_mut(0).unwrap().entry()?)
        .edit()?
        .as_bytes_mut()
        .copy_from_slice(b"edit");
    assert_eq!(names.read().get(0).unwrap()?, "edit");
    Ok(())
}
#[test]
fn occupied_old_layouts_only_upgrade_on_ensure() -> Result<()> {
    for far in [false, true] {
        for list in [false, true] {
            let mut arena = Arena::new(far);
            let counts = arena.counts.clone();
            let mut root = arena.root().init_struct(StructSize {
                data: 0,
                pointers: 1,
            });
            let old = StructSize {
                data: 1,
                pointers: 0,
            };
            if list {
                root.reborrow()
                    .get_pointer_field(0)
                    .init_struct_list(1, old)
                    .get_struct_element(0)
                    .set_data_field::<u64>(0, 7);
            } else {
                root.reborrow()
                    .get_pointer_field(0)
                    .init_struct(old)
                    .set_data_field::<u64>(0, 7);
            }
            fn strict<K: PointerType>(root: &mut StructBuilder<'_>, counts: &Counts) -> Result<()> {
                let allocations = counts.allocations.get();
                let e =
                    occupied(api::PointerField::<K>::new(root.reborrow(), 0, None, None).entry()?);
                let acquired = counts.accesses.get();
                assert_eq!(e.edit().err().unwrap().kind, ErrorKind::NeedsUpgrade);
                assert_eq!(counts.accesses.get(), acquired);
                assert_eq!(counts.allocations.get(), allocations);
                Ok(())
            }
            if list {
                strict::<api::List<NewRecord>>(&mut root, &counts)?;
                strict::<api::Generic<capnp::struct_list::Owned<new_record::Owned>>>(
                    &mut root, &counts,
                )?;
                let allocations = counts.allocations.get();
                let mut editor = occupied(
                    api::PointerField::<api::List<NewRecord>>::new(root.reborrow(), 0, None, None)
                        .entry()?,
                )
                .ensure()?;
                assert!(counts.allocations.get() > allocations);
                assert_eq!(editor.read().get(0).unwrap().value(), 7);
                editor.get_mut(0).unwrap().extra().copy_from("upgraded")?;
            } else {
                strict::<api::Struct<NewRecord>>(&mut root, &counts)?;
                strict::<api::Generic<new_record::Owned>>(&mut root, &counts)?;
                let allocations = counts.allocations.get();
                let mut editor = occupied(
                    api::PointerField::<api::Struct<NewRecord>>::new(
                        root.reborrow(),
                        0,
                        None,
                        None,
                    )
                    .entry()?,
                )
                .ensure()?;
                assert!(counts.allocations.get() > allocations);
                assert_eq!(editor.read().value(), 7);
                editor.extra().copy_from("upgraded")?;
            }
        }
        // Primitive-to-struct list conversion needs explicit upgrade even when
        // the virtual element sizes already contain all requested fields.
        let mut arena = Arena::new(far);
        let mut root = arena.root().init_struct(StructSize {
            data: 0,
            pointers: 1,
        });
        root.reborrow()
            .get_pointer_field(0)
            .init_list(ElementSize::EightBytes, 1);
        let slot = api::PointerField::<
            api::List<capntproto_test_support::field_api_capnp::api::OldRecord>,
        >::new(root.reborrow(), 0, None, None);
        assert_eq!(
            occupied(slot.entry()?).edit().err().unwrap().kind,
            ErrorKind::NeedsUpgrade
        );
    }
    Ok(())
}
#[test]
fn readonly_occupied_entries_cache_failure_without_copying_external_data() -> Result<()> {
    for far in [false, true] {
        let source: std::sync::Arc<[capnp::Word]> = capnp::Word::allocate_zeroed_vec(1).into();
        let weak = std::sync::Arc::downgrade(&source);
        let mut arena = Arena::new(far);
        let counts = arena.counts.clone();
        let mut root = arena.root().init_struct(StructSize {
            data: 0,
            pointers: 1,
        });
        use capnp::introspect::{Introspect, TypeVariant};
        let TypeVariant::Struct(raw) =
            capntproto_test_support::dynamic_test_capnp::external_case::Owned::introspect().which()
        else {
            unreachable!()
        };
        let schema = capnp::schema::StructSchema::new(raw);
        {
            let (mut external, token) =
                capnp::dynamic_struct::Builder::new(root.reborrow(), schema).with_orphanage();
            let value = token
                .in_struct(&mut external)?
                .reference_external_data(capnp::dynamic_orphan::ExternalData::new(source, 8)?)?;
            external.adopt_named("data", value).unwrap();
        }
        for ensure in [false, true] {
            let e = occupied(
                api::PointerField::<api::Data>::new(root.reborrow(), 0, None, None).entry()?,
            );
            let acquired = counts.accesses.get();
            let result = if ensure { e.ensure() } else { e.edit() };
            assert_eq!(result.err().unwrap().kind, ErrorKind::ReadOnlySegment);
            assert_eq!(counts.accesses.get(), acquired);
            let e = occupied(
                api::PointerField::<api::Generic<capnp::data::Owned>>::new(
                    root.reborrow(),
                    0,
                    None,
                    None,
                )
                .entry()?,
            );
            let acquired = counts.accesses.get();
            let result = if ensure { e.ensure() } else { e.edit() };
            assert_eq!(result.err().unwrap().kind, ErrorKind::ReadOnlySegment);
            assert_eq!(counts.accesses.get(), acquired);
        }
        assert_eq!(weak.strong_count(), 1);
        // Ordinary malformed pointer kinds fail during inspection, before any allocation.
        root.reborrow().get_pointer_field(0).init_data(2);
        let allocations = counts.allocations.get();
        assert!(
            api::PointerField::<api::Struct<NewRecord>>::new(root, 0, None, None)
                .entry()
                .is_err()
        );
        assert_eq!(counts.allocations.get(), allocations);
    }
    Ok(())
}

#[derive(serde::Deserialize)]
struct Step {
    action: String,
    state: Vec<u64>,
}
#[derive(serde::Deserialize)]
struct Trace {
    kind: String,
    layout: u64,
    steps: Vec<Step>,
}

fn replay_layout(trace: &Trace, far: bool) -> Result<()> {
    let mut arena = Arena::new(far);
    let counts = arena.counts.clone();
    let mut root = arena.root().init_struct(StructSize {
        data: 0,
        pointers: 1,
    });
    match trace.layout {
        0 => (),
        1 | 2 => root
            .reborrow()
            .get_pointer_field(0)
            .init_struct(StructSize {
                data: 1,
                pointers: u16::from(trace.layout == 1),
            })
            .set_data_field::<u64>(0, 7),
        3 => {
            root.reborrow().get_pointer_field(0).init_data(4);
        }
        _ => unreachable!(),
    }
    let initial_allocations = counts.allocations.get();
    let mut steps = trace.steps.iter().peekable();
    let step = steps.next().unwrap();
    assert_eq!(step.action, "inspect");
    {
        let field =
            api::PointerField::<api::Struct<NewRecord>>::new(root.reborrow(), 0, None, None);
        let entry = field.entry();
        assert_eq!(
            counts.allocations.get(),
            initial_allocations,
            "inspection allocated"
        );
        let mut editor = None;
        match entry {
            Err(_) => {
                assert_eq!(step.state[0], 5);
                assert_eq!(step.state[6], 1);
            }
            Ok(Entry::Vacant(v)) => {
                assert_eq!(step.state[0], 1);
                if let Some(next) = steps.next() {
                    match next.action.as_str() {
                        "initialize" => {
                            editor = Some(v.init()?);
                            assert_eq!(next.state[0], 3);
                            assert!(counts.allocations.get() > initial_allocations);
                        }
                        "drop" => {
                            assert_eq!(next.state[0], 4);
                        }
                        action => panic!("vacant {action}"),
                    }
                }
            }
            Ok(Entry::Occupied(v)) => {
                assert_eq!(step.state[0], 2);
                if let Some(next) = steps.next() {
                    let before = counts.accesses.get();
                    match next.action.as_str() {
                        "drop" => {
                            drop(v);
                            assert_eq!(next.state[0], 4);
                        }
                        "edit" | "ensure" => {
                            let strict = next.action == "edit";
                            let result = if strict { v.edit() } else { v.ensure() };
                            if strict || trace.layout == 1 {
                                assert_eq!(counts.accesses.get(), before, "repeated acquisition");
                            }
                            match result {
                                Ok(v) => {
                                    assert_eq!(next.state[0], 3);
                                    assert_eq!(v.read().value(), next.state[1]);
                                    assert_eq!(
                                        counts.allocations.get() > initial_allocations,
                                        next.state[3] != 0
                                    );
                                    editor = Some(v);
                                }
                                Err(e) => {
                                    assert_eq!(next.state[0], 5);
                                    assert_eq!(next.state[6], 2);
                                    assert_eq!(e.kind, ErrorKind::NeedsUpgrade);
                                }
                            }
                            if strict {
                                assert_eq!(counts.allocations.get(), initial_allocations);
                            }
                        }
                        action => panic!("occupied {action}"),
                    }
                }
            }
        }
        if let Some(mut editor) = editor {
            while let Some(next) = steps.peek() {
                match next.action.as_str() {
                    "write" => {
                        let next = steps.next().unwrap();
                        editor.value().set(9);
                        assert_eq!(editor.read().value(), next.state[1]);
                        assert_eq!(next.state[0], 3);
                    }
                    "drop" => {
                        assert_eq!(steps.next().unwrap().state[0], 4);
                        break;
                    }
                    action => panic!("editor {action}"),
                }
            }
        }
    }
    if let Some(next) = steps.next() {
        assert_eq!(next.action, "replace");
        api::PointerField::<api::Struct<NewRecord>>::new(root.reborrow(), 0, None, None)
            .replace()?
            .value()
            .set(11);
        assert_eq!(next.state[0], 6);
    }
    assert!(steps.next().is_none());
    let last = &trace.steps.last().unwrap().state;
    let pointer = root.as_reader().get_pointer_field(0);
    if last[2] == 0 {
        assert!(pointer.is_null());
    } else if last[2] == 3 {
        assert!(pointer.get_struct(None).is_err());
    } else {
        let value = pointer.get_struct(None)?;
        assert_eq!(value.get_data_field::<u64>(0), last[1]);
        assert_eq!(value.get_pointer_section_size(), u16::from(last[2] == 1));
    }
    Ok(())
}
thread_local! { static CAP_READS: Cell<u64> = const { Cell::new(0) }; }
struct CountedClient(capnp::capability::Client);
impl capnp::capability::FromClientHook for CountedClient {
    fn new(hook: Box<dyn capnp::private::capability::ClientHook>) -> Self {
        CAP_READS.with(|count| count.set(count.get() + 1));
        Self(capnp::capability::Client::new(hook))
    }
    fn into_client_hook(self) -> Box<dyn capnp::private::capability::ClientHook> {
        self.0.hook
    }
    fn as_client_hook(&self) -> &dyn capnp::private::capability::ClientHook {
        &*self.0.hook
    }
}
struct CapServer(Rc<Cell<u64>>);
impl capntproto_test_support::field_api_capnp::service::Server for CapServer {}
impl Drop for CapServer {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}
fn replay_caps(trace: &Trace, far: bool) -> Result<()> {
    let mut arena = Arena::new(far);
    let mut table = CapTable::new();
    let drops = Rc::new(Cell::new(0));
    let client: capntproto_test_support::field_api_capnp::service::Client =
        capnp_rpc::new_client(CapServer(drops.clone()));
    let identity = client.client.hook.get_ptr();
    let mut root = arena.root().init_struct(StructSize {
        data: 0,
        pointers: 1,
    });
    root.imbue(CapTableBuilder::Plain(&mut table));
    root.reborrow()
        .get_pointer_field(0)
        .set_capability(client.client.hook);
    CAP_READS.with(|c| c.set(0));
    let mut steps = trace.steps.iter().peekable();
    let mut held = None;
    if steps.peek().is_some_and(|s| s.action == "inspect") {
        let step = steps.next().unwrap();
        let entry = occupied(
            api::PointerField::<api::Capability<CountedClient>>::new(
                root.reborrow(),
                0,
                None,
                None,
            )
            .entry()?,
        );
        assert_eq!(CAP_READS.with(Cell::get), step.state[5]);
        assert_eq!(drops.get(), 1 - step.state[6]);
        if let Some(next) = steps.next() {
            match next.action.as_str() {
                "consume" => {
                    held = Some(entry.edit()?);
                    assert_eq!(held.as_ref().unwrap().0.hook.get_ptr(), identity);
                }
                "drop-entry" => drop(entry),
                action => panic!("entry {action}"),
            }
            assert_eq!(CAP_READS.with(Cell::get), next.state[5]);
            assert_eq!(drops.get(), 1 - next.state[6]);
        }
    }
    for step in steps {
        match step.action.as_str() {
            "clear" => root.reborrow().get_pointer_field(0).clear(),
            "drop-client" => {
                held = None;
            }
            action => panic!("cap {action}"),
        }
        assert_eq!(
            u64::from(!root.as_reader().get_pointer_field(0).is_null()),
            step.state[0]
        );
        assert_eq!(u64::from(held.is_some()), step.state[2]);
        assert_eq!(CAP_READS.with(Cell::get), step.state[5]);
        assert_eq!(drops.get(), 1 - step.state[6]);
        if let Some(cap) = &held {
            assert_eq!(cap.0.hook.get_ptr(), identity);
        }
    }
    Ok(())
}
#[test]
fn replay_tlc_field_entry_traces() -> Result<()> {
    let path = capntproto_test_support::verification::input("CAPNTPROTO_FIELD_ENTRY_TRACES")
        .expect("prepare verified trace corpus");
    let cases: Vec<Trace> = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    for trace in &cases {
        for far in [false, true] {
            if trace.kind == "caps" {
                replay_caps(trace, far)?;
            } else {
                replay_layout(trace, far)?;
            }
        }
    }
    Ok(())
}
#[test]
fn capability_entries_keep_the_acquired_hook_through_consumption_and_drop() -> Result<()> {
    for release_entry in [false, true] {
        let states = if release_entry {
            vec![
                ("inspect", vec![1, 1, 0, 1, 0, 1, 1, 0, 1]),
                ("drop-entry", vec![1, 0, 0, 1, 0, 1, 1, 0, 3]),
                ("clear", vec![0, 0, 0, 1, 0, 1, 0, 0, 4]),
            ]
        } else {
            vec![
                ("inspect", vec![1, 1, 0, 1, 0, 1, 1, 0, 1]),
                ("consume", vec![1, 0, 1, 1, 1, 1, 1, 0, 2]),
                ("clear", vec![0, 0, 1, 1, 1, 1, 1, 0, 4]),
                ("drop-client", vec![0, 0, 0, 1, 1, 1, 0, 0, 5]),
            ]
        };
        let trace = Trace {
            kind: "caps".into(),
            layout: 0,
            steps: states
                .into_iter()
                .map(|(a, s)| Step {
                    action: a.into(),
                    state: s,
                })
                .collect(),
        };
        replay_caps(&trace, false)?;
        replay_caps(&trace, true)?;
    }
    // Generated interface field entries use the same cache and preserve identity.
    let drops = Rc::new(Cell::new(0));
    let client: capntproto_test_support::field_api_capnp::service::Client =
        capnp_rpc::new_client(CapServer(drops.clone()));
    let identity = client.client.hook.get_ptr();
    let mut message = Message::<Types>::new()?;
    message.edit().cap().copy_from(client)?;
    let held = occupied(message.edit().cap().entry()?).ensure()?;
    message.edit().cap().clear();
    assert_eq!(held.client.hook.get_ptr(), identity);
    assert_eq!(drops.get(), 0);
    drop(held);
    assert_eq!(drops.get(), 1);
    Ok(())
}

#[test]
fn generic_occupied_upgrades_and_explicit_custom_acquisition_preserve_contents() -> Result<()> {
    for far in [false, true] {
        let mut arena = Arena::new(far);
        let counts = arena.counts.clone();
        let mut root = arena.root().init_struct(StructSize {
            data: 0,
            pointers: 1,
        });
        root.reborrow()
            .get_pointer_field(0)
            .init_struct(StructSize {
                data: 1,
                pointers: 0,
            })
            .set_data_field::<u64>(0, 7);
        let before = counts.allocations.get();
        let entry = occupied(
            api::PointerField::<api::Generic<new_record::Owned>>::new(
                root.reborrow(),
                0,
                None,
                None,
            )
            .entry()?,
        );
        assert_eq!(counts.allocations.get(), before);
        let mut editor = entry.ensure()?;
        assert!(counts.allocations.get() > before);
        assert_eq!(editor.reborrow().get_value(), 7);
        editor.set_extra("generic upgrade");
        assert_eq!(editor.reborrow_as_reader().get_extra()?, "generic upgrade");
        root.reborrow()
            .get_pointer_field(0)
            .init_struct_list(
                1,
                StructSize {
                    data: 1,
                    pointers: 0,
                },
            )
            .get_struct_element(0)
            .set_data_field::<u64>(0, 7);
        let before = counts.allocations.get();
        let entry = occupied(
            api::PointerField::<api::Generic<capnp::struct_list::Owned<new_record::Owned>>>::new(
                root.reborrow(),
                0,
                None,
                None,
            )
            .entry()?,
        );
        assert_eq!(counts.allocations.get(), before);
        let mut editor = entry.ensure()?;
        assert!(counts.allocations.get() > before);
        assert_eq!(editor.reborrow().get(0).get_value(), 7);
        editor.reborrow().get(0).set_extra("generic list upgrade");
        assert_eq!(
            editor.into_reader().get(0).get_extra()?,
            "generic list upgrade"
        );
    }
    // Custom pointer kinds explicitly participate in single-acquisition entries.
    struct CustomData;
    impl PointerType for CustomData {
        fn acquire<'a>(p: PointerBuilder<'a>) -> Result<api::OccupiedField<'a, Self>> {
            Ok(api::OccupiedField::from_acquired(Ok(Self::edit(p)?)))
        }
        type Ref<'a> = &'a [u8];
        type Mut<'a> = &'a mut [u8];
        fn read<'a>(
            p: capnp::private::layout::PointerReader<'a>,
            d: Option<&'a [capnp::Word]>,
        ) -> Result<Self::Ref<'a>> {
            api::Data::read(p, d)
        }
        fn init<'a>(p: PointerBuilder<'a>, n: usize) -> Result<Self::Mut<'a>> {
            api::Data::init(p, n)
        }
        fn edit<'a>(p: PointerBuilder<'a>) -> Result<Self::Mut<'a>> {
            api::Data::edit(p)
        }
        fn ensure<'a>(
            p: PointerBuilder<'a>,
            d: Option<&'a [capnp::Word]>,
        ) -> Result<Self::Mut<'a>> {
            api::Data::ensure(p, d)
        }
    }
    let mut arena = Arena::new(false);
    let mut root = arena.root().init_struct(StructSize {
        data: 0,
        pointers: 1,
    });
    root.reborrow()
        .get_pointer_field(0)
        .init_data(4)
        .copy_from_slice(b"data");
    let e = occupied(api::PointerField::<CustomData>::new(root.reborrow(), 0, None, None).entry()?);
    e.edit()?.copy_from_slice(b"edit");
    assert_eq!(
        root.as_reader().get_pointer_field(0).get_data(None)?,
        b"edit"
    );
    Ok(())
}
