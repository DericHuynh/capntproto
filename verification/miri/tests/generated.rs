use capnp::{
    field_api::{Message, MessageReader},
    message::{AllocationStrategy, HeapAllocator, ReaderOptions},
    ErrorKind, Result,
};
use capntproto_memory_checks::field_api_capnp::{
    api::{Future, Historical, StagedChoice},
    service,
};
use std::{cell::Cell, rc::Rc};

fn allocator(far: bool) -> HeapAllocator {
    if far {
        HeapAllocator::new()
            .first_segment_words(1)
            .allocation_strategy(AllocationStrategy::FixedSize)
    } else {
        HeapAllocator::new()
    }
}
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

#[test]
fn staged_groups_preserve_sibling_bits_and_release_caps_on_error_and_unwind() -> Result<()> {
    for far in [false, true] {
        for outcome in ["success", "error", "panic"] {
            let old = Rc::new(Cell::new(0));
            let new = Rc::new(Cell::new(0));
            let sibling = Rc::new(Cell::new(0));
            let mut message =
                Message::<StagedChoice<capnp::text::Owned>>::with_allocator(allocator(far))?;
            message.edit().sibling_bit().set(false);
            message.edit().sibling_byte().set(73);
            message.edit().sibling_text().copy_from("kept")?;
            message.edit().sibling_cap().copy_from(client(&sibling))?;
            message.edit().old().copy_from(client(&old))?;
            let address = message.read().sibling_text()?.as_ptr();
            let identity = message.read().sibling_cap()?.client.hook.get_ptr();
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                message.edit().details().replace_with(|mut group| {
                    assert!(group.read().flag());
                    assert_eq!(group.read().count(), 42);
                    group.flag().set(false);
                    group.count().set(99);
                    group.value().copy_from("generic".into())?;
                    group
                        .nested()
                        .first()
                        .replace()
                        .cap()
                        .copy_from(client(&new))?;
                    match outcome {
                        "success" => Ok(()),
                        "error" => Err(capnp::Error::failed("injected error".into())),
                        _ => panic!("injected unwind"),
                    }
                })
            }));
            assert_eq!(result.is_err(), outcome == "panic");
            if let Ok(result) = result {
                assert_eq!(result.is_ok(), outcome == "success");
            }
            assert!(!message.read().sibling_bit());
            assert_eq!(message.read().sibling_byte(), 73);
            assert_eq!(message.read().sibling_text()?.as_ptr(), address);
            assert_eq!(
                message.read().sibling_cap()?.client.hook.get_ptr(),
                identity
            );
            assert_eq!(sibling.get(), 0);
            if outcome == "success" {
                assert_eq!(old.get(), 1);
                assert_eq!(new.get(), 0);
                assert_eq!(message.read().details()?.count(), 99);
                assert_eq!(message.read().details()?.value()?, "generic");
                assert!(!message.read().details()?.flag());
            } else {
                assert_eq!(old.get(), 0);
                assert_eq!(new.get(), 1);
                let _ = message.read().old()?;
            }
            drop(message);
            assert_eq!(old.get(), 1);
            assert_eq!(new.get(), 1);
            assert_eq!(sibling.get(), 1);
        }
    }
    Ok(())
}

#[test]
fn generated_list_upgrade_preserves_elements_and_isolates_the_source() -> Result<()> {
    for far in [false, true] {
        let mut old = Message::<Historical>::with_allocator(allocator(far))?;
        old.edit().records().init_with(2, |i, mut record| {
            record.value().set(10 + i as u64);
            Ok(())
        })?;
        let (storage, _) = old.into_parts();
        let segments = storage.get_segments_for_output();
        let source =
            MessageReader::<Future, _>::from_segments(&segments[..], ReaderOptions::new())?;
        let mut destination = Message::<Future>::with_allocator(allocator(far))?;
        destination
            .edit()
            .records()
            .copy_from(source.read().records()?)?;
        let words = destination.size_in_words();
        assert_eq!(
            destination.edit().records().edit().err().unwrap().kind,
            ErrorKind::NeedsUpgrade
        );
        assert_eq!(destination.size_in_words(), words);
        destination
            .edit()
            .records()
            .ensure()?
            .get_mut(0)
            .unwrap()
            .extra()
            .copy_from("new child")?;
        assert_eq!(source.read().records()?.get(0).unwrap().extra()?, "");
        for index in 0..2 {
            assert_eq!(
                destination.read().records()?.get(index).unwrap().value(),
                10 + index as u64
            );
        }
        assert_eq!(destination.read().records()?.get(1).unwrap().extra()?, "");
        let address = destination
            .read()
            .records()?
            .get(0)
            .unwrap()
            .extra()?
            .as_ptr();
        let words = destination.size_in_words();
        destination
            .edit()
            .records()
            .ensure()?
            .get_mut(1)
            .unwrap()
            .value()
            .set(42);
        assert_eq!(destination.size_in_words(), words);
        assert_eq!(
            destination
                .read()
                .records()?
                .get(0)
                .unwrap()
                .extra()?
                .as_ptr(),
            address
        );
        assert_eq!(destination.read().records()?.get(1).unwrap().value(), 42);
    }
    Ok(())
}
