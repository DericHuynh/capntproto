use capnp::{capability::Promise, Error};
use std::{cell::Cell, rc::Rc};

/// Connection-wide send-buffer hint for streaming RPC. Matches C++ two-party
/// flow control: sample on sends/successful acks while running and in-flight
/// bytes exceed the largest message. After the first unavailable result, use
/// 64 KiB for every stream on this connection. A size change
/// alone does not wake blocked senders. Zero is a valid reported size.
///
/// Transport adapters should capture a weak socket reference in the query:
/// controllers and their pending acknowledgements may outlive transport IO.
#[derive(Clone)]
pub struct SendBufferWindow(Rc<State>);

struct State {
    unavailable: Cell<bool>,
    query: Box<dyn Fn() -> Option<usize>>,
}

impl SendBufferWindow {
    pub fn new(query: impl Fn() -> Option<usize> + 'static) -> Self {
        Self(Rc::new(State {
            unavailable: Cell::new(false),
            query: Box::new(query),
        }))
    }

    pub(super) fn get(&self) -> usize {
        if !self.0.unavailable.get() {
            if let Some(size) = (self.0.query)() {
                return size;
            }
            self.0.unavailable.set(true);
        }
        crate::flow_control::DEFAULT_WINDOW_SIZE
    }

    /// Independent credit accounting with a shared transport query/fallback.
    pub fn new_stream(&self) -> (Box<dyn crate::FlowController>, Promise<(), Error>) {
        let window = self.clone();
        crate::new_variable_window_flow_controller(move || window.get())
    }
}
