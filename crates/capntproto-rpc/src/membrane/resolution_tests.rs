use super::*;
use std::cell::Cell;

// A portable hook can change from pending to settled without changing its
// identity. This fixture also supplies a deliberately cyclic negative control.
struct InPlace {
    remaining: Rc<Cell<Option<usize>>>,
}
impl ClientHook for InPlace {
    fn add_ref(&self) -> Box<dyn ClientHook> {
        Box::new(Self {
            remaining: self.remaining.clone(),
        })
    }
    fn new_call(
        &self,
        _: u64,
        _: u16,
        _: Option<capnp::MessageSize>,
    ) -> Request<any_pointer::Owned, any_pointer::Owned> {
        unreachable!("resolution-only fixture")
    }
    fn call(
        &self,
        _: u64,
        _: u16,
        _: Box<dyn ParamsHook>,
        _: Box<dyn ResultsHook>,
    ) -> Promise<(), Error> {
        unreachable!("resolution-only fixture")
    }
    fn get_brand(&self) -> usize {
        0
    }
    fn get_ptr(&self) -> usize {
        Rc::as_ptr(&self.remaining) as usize
    }
    fn get_resolved(&self) -> Option<Box<dyn ClientHook>> {
        (self.remaining.get() == Some(0)).then(|| self.add_ref())
    }
    fn when_more_resolved(&self) -> Option<Promise<Box<dyn ClientHook>, Error>> {
        if let Some(remaining) = self.remaining.get() {
            self.remaining.set(Some(remaining.checked_sub(1)?));
        }
        Some(Promise::ok(self.add_ref()))
    }
    fn when_resolved(&self) -> Promise<(), Error> {
        crate::rpc::default_when_resolved_impl(self)
    }
}
struct Allow;
impl Policy for Allow {
    fn call(&self, _: Direction, _: u64, _: u16, _: &Client) -> capnp::Result<Option<Client>> {
        Ok(None)
    }
}
#[test]
fn in_place_resolution_settles_without_retaining_membrane_cycles() {
    for wrapped in [false, true] {
        let state = Rc::new(Cell::new(Some(1)));
        let weak = Rc::downgrade(&state);
        let hook: Box<dyn ClientHook> = Box::new(InPlace { remaining: state });
        let membrane = Membrane::new(Rc::new(Allow));
        let hook = if wrapped {
            membrane.wrap(hook, false)
        } else {
            hook
        };
        let identity = hook.get_ptr();
        let settled = futures::executor::block_on(crate::rpc::join::settle(hook)).unwrap();
        assert_eq!(settled.get_ptr(), identity);
        assert!(settled.when_more_resolved().is_none());
        if wrapped {
            assert!(settled.get_resolved().is_none());
        }
        drop(settled);
        // The cache contains only weak wrappers, even while the membrane lives.
        assert!(weak.upgrade().is_none());
        drop(membrane);
    }
}
#[test]
fn continuing_resolution_cycle_is_rejected_and_released() {
    let state = Rc::new(Cell::new(None));
    let weak = Rc::downgrade(&state);
    let result = futures::executor::block_on(crate::rpc::join::settle(Box::new(InPlace {
        remaining: state,
    })));
    let error = result.err().expect("unresolved cycle must reject");
    assert!(error.to_string().contains("cyclic"));
    assert!(weak.upgrade().is_none());
}
