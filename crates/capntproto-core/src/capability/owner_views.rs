use crate::{
    any_pointer,
    capability::{Params, Response},
    message, text, Error, ErrorKind,
};
use core::cell::Cell;

struct Segment<'a> {
    bytes: &'a [u8],
    drops: &'a Cell<usize>,
}
impl message::ReaderSegments for Segment<'_> {
    fn get_segment(&self, id: u32) -> Option<&[u8]> {
        (id == 0).then_some(self.bytes)
    }
    fn len(&self) -> usize {
        1
    }
}
impl Drop for Segment<'_> {
    fn drop(&mut self) {
        self.drops.set(self.drops.get() + 1);
    }
}

#[test]
fn concrete_parameter_and_response_owners_retain_bytes_and_drop_once_without_allocating() {
    let drops = Cell::new(0);
    let measured = allocation_counter::measure(|| {
        let mut words = [crate::word(0, 0, 0, 0, 0, 0, 0, 0); 4];
        let mut message = message::FixedBuilder::new(&mut words).unwrap();
        message.try_set_root_text("legacy").unwrap();
        let bytes = message.into_segment();
        let reader = message::Reader::new(
            Segment {
                bytes,
                drops: &drops,
            },
            Default::default(),
        );
        let params =
            Params::<any_pointer::Owned, _>::from_owner(reader).into_typed::<text::Owned>();
        assert_eq!(params.get().unwrap(), "legacy");
        let response =
            Response::<any_pointer::Owned, _>::from_owner(params.hook).into_typed::<text::Owned>();
        assert_eq!(drops.get(), 0);
        assert_eq!(response.get().unwrap(), "legacy");
        drop(response);
        assert_eq!(drops.get(), 1);
    });
    assert_eq!(measured.count_total, 0);
    assert_eq!(measured.bytes_total, 0);
}

#[test]
fn moved_selected_payloads_preserve_the_original_traversal_budget() {
    let drops = Cell::new(0);
    let measured = allocation_counter::measure(|| {
        let mut words = [crate::word(0, 0, 0, 0, 0, 0, 0, 0); 4];
        let mut message = message::FixedBuilder::new(&mut words).unwrap();
        message.try_set_root_text("legacy").unwrap();
        let bytes = message.into_segment();
        let mut options = message::ReaderOptions::new();
        options.traversal_limit_in_words(Some(3));
        let reader = message::Reader::new(
            Segment {
                bytes,
                drops: &drops,
            },
            options,
        );
        let (selected, ()) = reader
            .try_select_pointer::<_, Error>(|root| Ok(Some((root, ()))))
            .unwrap()
            .unwrap();
        let params = Params::<text::Owned, _>::from_owner(selected);
        assert_eq!(params.get().unwrap(), "legacy");
        let moved = params.into_typed::<any_pointer::Owned>();
        let response = Response::<text::Owned, _>::from_owner(moved.hook);
        assert_eq!(response.get().unwrap(), "legacy");
        assert_eq!(
            response.get().unwrap_err().kind,
            ErrorKind::ReadLimitExceeded
        );
        drop(response);
        assert_eq!(drops.get(), 1);
    });
    assert_eq!(measured.count_total, 0);
    assert_eq!(measured.bytes_total, 0);
}

#[test]
fn externally_owned_read_budget_is_not_replenished_by_a_new_wrapper() {
    let mut words = [crate::word(0, 0, 0, 0, 0, 0, 0, 0); 4];
    let mut message = message::FixedBuilder::new(&mut words).unwrap();
    message.try_set_root_text("legacy").unwrap();
    let bytes = message.into_segment();
    let segments = [bytes];
    let budget = message::ReadBudget::new(Some(2));
    let measured = allocation_counter::measure(|| {
        let reader = message::Reader::with_budget(&segments[..], Default::default(), &budget);
        let params = Params::<text::Owned, _>::from_owner(reader);
        assert_eq!(params.get().unwrap(), "legacy");
        let response = Response::<text::Owned, _>::from_owner(params.hook);
        assert_eq!(budget.remaining_words(), Some(0));
        assert_eq!(
            response.get().unwrap_err().kind,
            ErrorKind::ReadLimitExceeded
        );
    });
    assert_eq!(measured.count_total, 0);
    assert_eq!(measured.bytes_total, 0);
}

#[cfg(feature = "alloc")]
#[test]
fn boxed_default_owners_retain_two_word_layout_and_forward_parameter_identity() {
    use crate::private::capability::{ParamsHook, ResponseHook};
    struct Hook;
    impl ParamsHook for Hook {
        fn get(&self) -> crate::Result<any_pointer::Reader<'_>> {
            Err(Error::from_kind(ErrorKind::Failed))
        }
        fn as_any(&self) -> Option<&dyn core::any::Any> {
            Some(self)
        }
    }
    impl ResponseHook for Hook {
        fn get(&self) -> crate::Result<any_pointer::Reader<'_>> {
            Err(Error::from_kind(ErrorKind::Failed))
        }
    }
    let params = Params::<text::Owned>::new(alloc::boxed::Box::new(Hook));
    let response = Response::<any_pointer::Owned>::new(alloc::boxed::Box::new(Hook));
    let measured = allocation_counter::measure(|| {
        assert!(params.hook.as_any().unwrap().is::<Hook>());
        assert_eq!(params.get().unwrap_err().kind, ErrorKind::Failed);
        assert!(response.get().is_err());
    });
    assert_eq!(
        core::mem::size_of_val(&params),
        2 * core::mem::size_of::<usize>()
    );
    assert_eq!(
        core::mem::size_of_val(&response),
        2 * core::mem::size_of::<usize>()
    );
    assert_eq!(measured.count_total, 0);
    assert_eq!(measured.bytes_total, 0);
}
