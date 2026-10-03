use capnp::field_api::Message;
use capntproto_test_support::field_api_capnp::{
    api::{ScalarGroup, StagedChoice, StagedChoiceUnionTag},
    service,
};
use std::{cell::Cell, panic::AssertUnwindSafe, rc::Rc};

type Choice = StagedChoice<capnp::text::Owned>;
struct Server(Rc<Cell<usize>>);
impl service::Server for Server {}
impl Drop for Server {
    fn drop(&mut self) {
        self.0.set(self.0.get() + 1);
    }
}
fn client(drops: &Rc<Cell<usize>>) -> service::Client {
    capnp_rpc::new_client(Server(drops.clone()))
}
fn allocator(far: bool) -> capnp::message::HeapAllocator {
    if far {
        capnp::message::HeapAllocator::new()
            .first_segment_words(1)
            .allocation_strategy(capnp::message::AllocationStrategy::FixedSize)
    } else {
        capnp::message::HeapAllocator::new()
    }
}

#[test]
fn group_callbacks_publish_atomically_and_preserve_siblings_and_payload_addresses(
) -> capnp::Result<()> {
    for far in [false, true] {
        for existing_group in [false, true] {
            for outcome in ["success", "error", "panic"] {
                let drops: [_; 4] = std::array::from_fn(|_| Rc::new(Cell::new(0)));
                let mut message = Message::<Choice>::with_allocator(allocator(far))?;
                {
                    let mut root = message.edit();
                    root.sibling_bit().set(false);
                    root.sibling_byte().set(73);
                    root.sibling_text().copy_from("sibling")?;
                    root.sibling_cap().copy_from(client(&drops[3]))?;
                    if existing_group {
                        let mut group = root.details().replace();
                        group.count().set(123);
                        group.label().copy_from("old label")?;
                        group
                            .nested()
                            .first()
                            .replace()
                            .cap()
                            .copy_from(client(&drops[0]))?;
                    } else {
                        root.old().copy_from(client(&drops[0]))?;
                    }
                }
                let sibling_address = message.read().sibling_text()?.as_ptr();
                let sibling_cap = message.read().sibling_cap()?.client.hook.get_ptr();
                let old_bytes = message.compact_copy()?.to_vec();
                let mut label_address = core::ptr::null();
                let mut generic_address = core::ptr::null();
                let mut identity = 0;
                let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
                    message.edit().details().replace_with(|mut group| {
                        assert!(group.read().flag());
                        assert_eq!(group.read().count(), 42);
                        assert_eq!(group.read().label()?, "default label");
                        group.flag().set(false);
                        group.count().set(987);
                        group.label().copy_from("new label")?;
                        group.value().copy_from("generic text".into())?;
                        label_address = group.read().label()?.as_ptr();
                        generic_address = group.read().value()?.as_bytes().as_ptr();
                        group
                            .nested()
                            .first()
                            .replace()
                            .cap()
                            .copy_from(client(&drops[1]))?;
                        let cap = client(&drops[2]);
                        identity = cap.client.hook.get_ptr();
                        group.nested().second().replace_with(|mut nested| {
                            assert_eq!(nested.read().number(), -1.5);
                            nested.cap().copy_from(cap)?;
                            nested.number().set(8.25);
                            Ok(())
                        })?;
                        assert_eq!(drops[1].get(), 1);
                        assert_eq!(drops[0].get(), 0);
                        match outcome {
                            "error" => Err(capnp::Error::failed("construction failed".into())),
                            "panic" => panic!("construction panicked"),
                            _ => Ok(()),
                        }
                    })
                }));
                assert_eq!(result.is_err(), outcome == "panic");
                if let Ok(result) = result {
                    assert_eq!(result.is_err(), outcome == "error");
                }
                assert!(!message.read().sibling_bit());
                assert_eq!(message.read().sibling_byte(), 73);
                assert_eq!(message.read().sibling_text()?.as_ptr(), sibling_address);
                assert_eq!(
                    message.read().sibling_cap()?.client.hook.get_ptr(),
                    sibling_cap
                );
                assert_eq!(drops[3].get(), 0);
                if outcome == "success" {
                    assert_eq!(drops[0].get(), 1);
                    assert_eq!(drops[2].get(), 0);
                    let group = message.read().details()?;
                    assert!(!group.flag());
                    assert_eq!(group.count(), 987);
                    assert_eq!(group.label()?.as_ptr(), label_address);
                    assert_eq!(group.value()?.as_bytes().as_ptr(), generic_address);
                    let nested = group.nested();
                    let capntproto_test_support::field_api_capnp::api::staged_choice::details::NestedUnionRef::Second(nested) = nested? else { panic!("wrong nested arm") };
                    assert_eq!(nested.cap()?.client.hook.get_ptr(), identity);
                    assert_eq!(nested.number(), 8.25);
                    // Replacing an already selected group starts from defaults.
                    message.edit().details().replace_with(|_| Ok(()))?;
                    assert_eq!(drops[2].get(), 1);
                    assert_eq!(message.read().details()?.count(), 42);
                    assert_eq!(message.read().details()?.label()?, "default label");
                } else {
                    assert_eq!(drops[0].get(), 0);
                    assert_eq!(drops[2].get(), 1);
                    // Ignore unreachable allocation history and compare the
                    // attached root/descendants with freshly assigned cap slots.
                    assert_eq!(message.compact_copy()?.to_vec(), old_bytes);
                    if existing_group {
                        assert_eq!(message.read().details()?.count(), 123);
                        assert_eq!(message.read().details()?.label()?, "old label");
                    }
                }
                drop(message);
                assert!(drops.iter().all(|d| d.get() == 1));
            }
        }
        let mut scalar = Message::<ScalarGroup>::with_allocator(allocator(far))?;
        scalar.edit().sibling().set(false);
        scalar.edit().details().replace_with(|mut g| {
            assert!(g.read().flag());
            g.flag().set(false);
            Ok(())
        })?;
        assert!(!scalar.read().sibling());
        assert!(!scalar.read().details()?.flag());
    }
    Ok(())
}

#[derive(serde::Deserialize)]
struct Trace {
    steps: Vec<Step>,
}
#[derive(serde::Deserialize)]
struct Step {
    action: String,
    state: Vec<u8>,
}
type GroupRef<'a> = capntproto_test_support::field_api_capnp::api::staged_choice::DetailsRef<
    'a,
    capnp::text::Owned,
>;
fn group_cap(group: GroupRef<'_>) -> capnp::Result<Option<(u8, service::Client)>> {
    use capntproto_test_support::field_api_capnp::api::staged_choice::details::NestedUnionRef;
    Ok(match group.nested()? {
        NestedUnionRef::None => None,
        NestedUnionRef::First(g) => Some((1, g.cap()?)),
        NestedUnionRef::Second(g) => Some((2, g.cap()?)),
        _ => panic!("unexpected group tag"),
    })
}
fn check_ownership(
    step: &Step,
    drops: &[Rc<Cell<usize>>; 4],
    held: &Option<(u8, service::Client)>,
    identities: &[usize; 2],
) {
    let s = &step.state;
    assert_eq!(u8::from(drops[0].get() == 0), s[4], "old: {}", step.action);
    for i in 0..2 {
        assert_eq!(
            u8::from(s[7 + i] != 0 && drops[1 + i].get() == 0),
            s[5 + i],
            "candidate {i}: {}",
            step.action
        );
    }
    assert_eq!(held.as_ref().map_or(0, |h| h.0), s[3]);
    if let Some((id, cap)) = held {
        assert_eq!(cap.client.hook.get_ptr(), identities[usize::from(*id - 1)]);
    }
    assert_eq!(u8::from(drops[3].get() == 0), s[12]);
}
fn replay(trace: &Trace, far: bool, existing_group: bool) -> capnp::Result<()> {
    let drops: [_; 4] = std::array::from_fn(|_| Rc::new(Cell::new(0)));
    let mut message = Message::<Choice>::with_allocator(allocator(far))?;
    let old = client(&drops[0]);
    let old_identity = old.client.hook.get_ptr();
    {
        let mut root = message.edit();
        root.sibling_bit().set(false);
        root.sibling_byte().set(73);
        root.sibling_text().copy_from("sibling")?;
        root.sibling_cap().copy_from(client(&drops[3]))?;
        if existing_group {
            let mut group = root.details().replace();
            group.count().set(9);
            group.nested().first().replace().cap().copy_from(old)?;
        } else {
            root.old().copy_from(old)?;
        }
    }
    let old_bytes = message.compact_copy()?.to_vec();
    let sibling_address = message.read().sibling_text()?.as_ptr();
    let mut identities = [0; 2];
    let mut held = None;
    let mut cursor = 0;
    let mut label_address = core::ptr::null();
    let mut terminal = None;
    let result = std::panic::catch_unwind(AssertUnwindSafe(|| {
        message.edit().details().replace_with(|mut group| {
            group.count().set(0);
            group.label().copy_from("staged payload")?;
            label_address = group.read().label()?.as_ptr();
            while cursor < trace.steps.len() {
                let step = &trace.steps[cursor];
                cursor += 1;
                match step.action.as_str() {
                    "stage" => (),
                    "write" => group.count().set(u64::from(step.state[2])),
                    "first" => {
                        let cap = client(&drops[1]);
                        identities[0] = cap.client.hook.get_ptr();
                        group.nested().first().replace().cap().copy_from(cap)?;
                    }
                    "second" => {
                        let cap = client(&drops[2]);
                        identities[1] = cap.client.hook.get_ptr();
                        group.nested().second().replace().cap().copy_from(cap)?;
                    }
                    "hold" => held = group_cap(group.read())?,
                    "release" => drop(held.take()),
                    "commit" | "error" | "panic" => {
                        terminal = Some(cursor - 1);
                        return match step.action.as_str() {
                            "commit" => Ok(()),
                            "error" => Err(capnp::Error::failed("source error".into())),
                            _ => panic!("source unwinding"),
                        };
                    }
                    other => panic!("unexpected callback action {other}"),
                }
                assert_eq!(group.read().count(), u64::from(step.state[2]));
                assert_eq!(group_cap(group.read())?.map_or(0, |c| c.0), step.state[1]);
                check_ownership(step, &drops, &held, &identities);
            }
            // A shortest prefix can end inside the callback. Inspect it above,
            // then abandon construction to release the exclusive parent borrow.
            Err(capnp::Error::failed("end of prefix".into()))
        })
    }));
    let outcome = terminal.map(|i| trace.steps[i].action.as_str());
    assert_eq!(result.is_err(), outcome == Some("panic"));
    if let Ok(result) = result {
        assert_eq!(result.is_ok(), outcome == Some("commit"));
    }
    if outcome != Some("commit") {
        assert_eq!(message.compact_copy()?.to_vec(), old_bytes);
    }
    if let Some(index) = terminal {
        for (i, step) in trace.steps.iter().enumerate().skip(index) {
            if i != index {
                match step.action.as_str() {
                    "hold" => held = group_cap(message.read().details()?)?,
                    "release" => drop(held.take()),
                    "clear" => message.edit().empty().set(),
                    _ => panic!("unexpected post-callback action"),
                }
            }
            check_ownership(step, &drops, &held, &identities);
            match step.state[9] {
                0 => {
                    let old = if existing_group {
                        let g = message.read().details()?;
                        assert_eq!(g.count(), 9);
                        group_cap(g)?.unwrap().1
                    } else {
                        message.read().old()?
                    };
                    assert_eq!(old.client.hook.get_ptr(), old_identity);
                }
                1 => {
                    let group = message.read().details()?;
                    assert_eq!(group.count(), u64::from(step.state[11]));
                    assert_eq!(group.label()?.as_ptr(), label_address);
                    let cap = group_cap(group)?;
                    assert_eq!(cap.as_ref().map_or(0, |c| c.0), step.state[10]);
                    if let Some((id, cap)) = cap {
                        assert_eq!(cap.client.hook.get_ptr(), identities[usize::from(id - 1)]);
                    }
                }
                2 => assert_eq!(message.read().tag(), StagedChoiceUnionTag::Empty),
                _ => panic!("unknown model tag"),
            }
            assert!(!message.read().sibling_bit());
            assert_eq!(message.read().sibling_byte(), 73);
            assert_eq!(message.read().sibling_text()?.as_ptr(), sibling_address);
        }
    }
    drop(held);
    drop(message);
    let last = &trace.steps.last().unwrap().state;
    assert_eq!(drops[0].get(), 1);
    assert_eq!(drops[1].get(), usize::from(last[7]));
    assert_eq!(drops[2].get(), usize::from(last[8]));
    assert_eq!(drops[3].get(), 1);
    Ok(())
}

#[test]
fn replay_tlc_group_staging_traces() -> capnp::Result<()> {
    let path = capntproto_test_support::verification::input("CAPNTPROTO_GROUP_STAGING_TRACES")
        .expect("prepare verified trace corpus");
    let traces: Vec<Trace> = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert!(!traces.is_empty());
    for far in [false, true] {
        for existing_group in [false, true] {
            for trace in &traces {
                replay(trace, far, existing_group)?;
            }
        }
    }
    Ok(())
}
