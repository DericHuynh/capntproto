use super::*;
use crate::rpc_capnp::{self, api};
use capnp::{capability, field_api::Struct, message};
use core::cell::Cell;
const ZERO: capnp::Word = capnp::word(0, 0, 0, 0, 0, 0, 0, 0);
struct Segments<'a> {
    bytes: &'a [u8],
    drops: &'a Cell<usize>,
}
impl message::ReaderSegments for Segments<'_> {
    fn len(&self) -> usize {
        1
    }
    fn get_segment(&self, id: u32) -> Option<&[u8]> {
        (id == 0).then_some(self.bytes)
    }
}
impl Drop for Segments<'_> {
    fn drop(&mut self) {
        self.drops.set(self.drops.get() + 1);
    }
}
struct Caps<'a> {
    frame: &'a Cell<usize>,
    drops: &'a Cell<usize>,
    expected: usize,
}
impl Capabilities for Caps<'_> {
    fn bind<'a>(&'a self, content: any_pointer::Reader<'a>) -> any_pointer::Reader<'a> {
        content
    }
}
impl Drop for Caps<'_> {
    fn drop(&mut self) {
        assert_eq!(self.frame.get(), self.expected);
        self.drops.set(self.drops.get() + 1);
    }
}
fn frame<'a>(words: &'a mut [capnp::Word], returned: bool) -> message::FixedBuilder<'a> {
    let mut builder = message::FixedBuilder::new(words).unwrap();
    let root = builder.try_init_root::<api::Message>().unwrap();
    let content = if returned {
        root.into_field(api::Message::RETURN)
            .init()
            .unwrap()
            .into_field(api::Return::RESULTS)
            .init()
            .unwrap()
            .into_field(api::Payload::CONTENT)
    } else {
        root.into_field(api::Message::CALL)
            .init()
            .unwrap()
            .into_field(api::Call::PARAMS)
            .init()
            .unwrap()
            .into_field(api::Payload::CONTENT)
    };
    content
        .cast::<Struct<api::Exception>>()
        .init()
        .unwrap()
        .field(api::Exception::REASON)
        .copy_from("retained")
        .unwrap();
    builder
}
#[test]
fn concrete_legacy_params_and_response_retain_frame_and_capability_drop_order() {
    let measured = allocation_counter::measure(|| {
        let drops = Cell::new(0);
        let caps = Cell::new(0);
        let mut words = [ZERO; 32];
        let source = frame(&mut words, false);
        let input = message::Reader::new(
            Segments {
                bytes: source.into_segment(),
                drops: &drops,
            },
            Default::default(),
        );
        let owner = Params::new(
            input,
            Caps {
                frame: &drops,
                drops: &caps,
                expected: 1,
            },
        );
        let params = capability::Params::<rpc_capnp::exception::Owned, _>::from_owner(owner);
        assert_eq!(params.get().unwrap().get_reason().unwrap(), "retained");
        assert_eq!(drops.get(), 0);
        drop(params);
        assert_eq!(drops.get(), 1);
        assert_eq!(caps.get(), 1);
        let mut words = [ZERO; 32];
        let source = frame(&mut words, true);
        let input = message::Reader::new(
            Segments {
                bytes: source.into_segment(),
                drops: &drops,
            },
            Default::default(),
        );
        let owner = Response::new(
            input,
            Caps {
                frame: &drops,
                drops: &caps,
                expected: 2,
            },
        );
        let response = capability::Response::<rpc_capnp::exception::Owned, _>::from_owner(owner);
        assert_eq!(response.get().unwrap().get_reason().unwrap(), "retained");
        drop(response);
        assert_eq!(drops.get(), 2);
        assert_eq!(caps.get(), 2);
    });
    assert_eq!((measured.count_total, measured.bytes_total), (0, 0));
}

struct Cached<'a> {
    root: any_pointer::Reader<'a>,
    payload: any_pointer::Reader<'a>,
    bodies: Cell<usize>,
    params: Cell<usize>,
    results: Cell<usize>,
    cached: Cell<usize>,
}
impl IncomingMessage for Cached<'_> {
    fn size_in_words(&self) -> usize {
        8
    }
    fn get_body(&self) -> capnp::Result<any_pointer::Reader<'_>> {
        self.bodies.set(self.bodies.get() + 1);
        Ok(self.root)
    }
    fn get_params(&self) -> capnp::Result<any_pointer::Reader<'_>> {
        self.params.set(self.params.get() + 1);
        Ok(self.payload)
    }
    fn get_results(&self) -> capnp::Result<any_pointer::Reader<'_>> {
        self.results.set(self.results.get() + 1);
        Ok(self.payload)
    }
    fn cache_params(&self, _: rpc_capnp::payload::Reader<'_>) -> capnp::Result<()> {
        self.cached.set(self.cached.get() + 1);
        Ok(())
    }
    fn cache_results(&self, _: rpc_capnp::payload::Reader<'_>) -> capnp::Result<()> {
        self.cached.set(self.cached.get() + 1);
        Ok(())
    }
}
#[test]
fn borrowed_cached_transport_payload_access_does_not_redecode_the_envelope() {
    let measured = allocation_counter::measure(|| {
        // Each cache belongs to its own immutable external frame/reader.
        for returned in [false, true] {
            let mut words = [ZERO; 32];
            let source = frame(&mut words, returned);
            let reader = message::Reader::new(&source, Default::default());
            let root = reader.get_root::<any_pointer::Reader<'_>>().unwrap();
            let parsed = root.get_as::<rpc_capnp::message::Reader<'_>>().unwrap();
            let payload = match parsed.which().unwrap() {
                rpc_capnp::message::Call(call) => call.unwrap().get_params().unwrap(),
                rpc_capnp::message::Return(ret) => {
                    let rpc_capnp::return_::Results(payload) = ret.unwrap().which().unwrap() else {
                        panic!("results lost")
                    };
                    payload.unwrap()
                }
                _ => panic!("wrong envelope"),
            };
            let cached = Cached {
                root,
                payload: payload.get_content(),
                bodies: Cell::new(0),
                params: Cell::new(0),
                results: Cell::new(0),
                cached: Cell::new(0),
            };
            if returned {
                let owner = Response::new(&cached, ());
                let response =
                    capability::Response::<rpc_capnp::exception::Owned, _>::from_owner(owner);
                assert_eq!(response.get().unwrap().get_reason().unwrap(), "retained");
                <&Cached<'_> as IncomingMessage>::cache_results(&&cached, payload).unwrap();
                assert_eq!(cached.results.get(), 1);
            } else {
                let owner = Params::new(&cached, ());
                let params =
                    capability::Params::<rpc_capnp::exception::Owned, _>::from_owner(owner);
                assert_eq!(params.get().unwrap().get_reason().unwrap(), "retained");
                <&Cached<'_> as IncomingMessage>::cache_params(&&cached, payload).unwrap();
                assert_eq!(cached.params.get(), 1);
            }
            assert_eq!(cached.bodies.get(), 0);
            assert_eq!(cached.cached.get(), 1);
        }
    });
    assert_eq!((measured.count_total, measured.bytes_total), (0, 0));
}
#[test]
fn moving_whole_frame_owners_never_replenishes_the_shared_read_budget() {
    let measured = allocation_counter::measure(|| {
        let mut words = [ZERO; 32];
        let source = frame(&mut words, false);
        let budget = message::ReadBudget::new(Some(128));
        let reader = message::Reader::with_budget(&source, Default::default(), &budget);
        let mut params = capability::Params::<rpc_capnp::exception::Owned, _>::from_owner(
            Params::new(reader, ()),
        );
        assert_eq!(params.get().unwrap().get_reason().unwrap(), "retained");
        let mut previous = budget.remaining_words().unwrap();
        assert!(previous < 128);
        let mut failed = false;
        for _ in 0..64 {
            let owner = params.hook;
            let (reader, caps) = owner.into_owners();
            params = capability::Params::from_owner(Params::new(reader, caps));
            match params.get() {
                Ok(view) => {
                    let _ = view.get_reason();
                }
                Err(_) => {
                    failed = true;
                    break;
                }
            }
            let remaining = budget.remaining_words().unwrap();
            assert!(remaining < previous);
            previous = remaining;
        }
        assert!(failed, "reopening an owner reset the message budget");
    });
    assert_eq!((measured.count_total, measured.bytes_total), (0, 0));
}
#[test]
fn wrong_envelope_or_return_variant_is_not_exposed_as_successful_payload() {
    let mut words = [ZERO; 32];
    let source = frame(&mut words, false);
    let reader = message::Reader::new(&source, Default::default());
    assert!(Response::new(reader, ()).get().is_err());
    let mut words = [ZERO; 32];
    let source = frame(&mut words, true);
    let reader = message::Reader::new(&source, Default::default());
    assert!(Params::new(reader, ()).get().is_err());
    let mut words = [ZERO; 32];
    let mut source = message::FixedBuilder::new(&mut words).unwrap();
    source
        .try_init_root::<api::Message>()
        .unwrap()
        .into_field(api::Message::RETURN)
        .init()
        .unwrap()
        .into_field(api::Return::EXCEPTION)
        .init()
        .unwrap();
    let reader = message::Reader::new(&source, Default::default());
    assert!(Response::new(reader, ()).get().is_err());
}

#[cfg(feature = "alloc")]
#[test]
fn boxed_frame_forwarding_preserves_its_cached_payload_path() {
    let mut words = [ZERO; 32];
    let source = frame(&mut words, false);
    let reader = message::Reader::new(&source, Default::default());
    let root = reader.get_root::<any_pointer::Reader<'_>>().unwrap();
    let parsed = root.get_as::<rpc_capnp::message::Reader<'_>>().unwrap();
    let rpc_capnp::message::Call(call) = parsed.which().unwrap() else {
        panic!("Call lost")
    };
    let cached = alloc::boxed::Box::new(Cached {
        root,
        payload: call.unwrap().get_params().unwrap().get_content(),
        bodies: Cell::new(0),
        params: Cell::new(0),
        results: Cell::new(0),
        cached: Cell::new(0),
    });
    let measured = allocation_counter::measure(|| {
        let params = capability::Params::<rpc_capnp::exception::Owned, _>::from_owner(Params::new(
            &cached,
            (),
        ));
        assert_eq!(params.get().unwrap().get_reason().unwrap(), "retained");
    });
    assert_eq!((measured.count_total, measured.bytes_total), (0, 0));
    assert_eq!(cached.params.get(), 1);
    assert_eq!(cached.bodies.get(), 0);
}
