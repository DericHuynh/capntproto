//! Caller-backed legacy question record, retained response and deferred Finish.
#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]
use capnp::message;
use capntproto_rpc_engine::completion::direct;
use capntproto_rpc_engine::question::{
    reference::Reference,
    release::{self, Last, Owner, Publication},
    Finish, Question, Release,
};
use capntproto_rpc_wire::rpc_capnp::{self, api};
use core::{
    cell::{Cell, Ref, RefCell, RefMut},
    future::{Future, Ready},
    ops::Deref,
    pin::Pin,
    task::{Context as TaskContext, Poll, Waker},
};
const ZERO: capnp::Word = capnp::word(0, 0, 0, 0, 0, 0, 0, 0);
struct Context<'a> {
    question: RefCell<Option<Question>>,
    // Internal owners are keys. The external public lease borrows Context;
    // storing that lease here instead would make the context self-referential.
    record: RefCell<Reference<u32, u32, (), Ready<&'a [u8]>, u32>>,
    pending: Cell<Option<(Finish, bool)>>,
    output: RefCell<message::FixedBuilder<'a>>,
    capacity: Cell<bool>,
    published: Cell<bool>,
}
impl<'a> direct::Owner<Ready<&'a [u8]>> for Context<'a> {
    fn reply(&self) -> Ref<'_, direct::Reply<Ready<&'a [u8]>>> {
        Ref::map(self.record.borrow(), Reference::reply)
    }
    fn reply_mut(&self) -> RefMut<'_, direct::Reply<Ready<&'a [u8]>>> {
        RefMut::map(self.record.borrow_mut(), Reference::reply_mut)
    }
}
impl Owner for Context<'_> {
    type Key = u32;
    fn release(&self, key: &u32) -> Option<Release> {
        assert_eq!(*key, 37);
        assert!(self.record.try_borrow_mut().is_ok());
        self.question.borrow_mut().as_mut().map(Question::release)
    }
    fn finish(&self, key: &u32, finish: Finish, reclaim: bool) -> Publication {
        assert!(self.question.try_borrow_mut().is_ok());
        assert!(self.question.borrow().is_some());
        if !self.capacity.get() {
            self.pending.set(Some((finish, reclaim)));
            return Publication::Deferred;
        }
        let mut output = self.output.borrow_mut();
        let root = output.try_init_root::<api::Message>().unwrap();
        let mut frame = root.into_field(api::Message::FINISH).init().unwrap();
        frame.scalar(api::Finish::QUESTION_ID).set(*key);
        frame
            .scalar(api::Finish::RELEASE_RESULT_CAPS)
            .set(finish.release_result_caps);
        self.published.set(true);
        Publication::Accepted
    }
    fn reclaim(&self, key: &u32) {
        assert_eq!(*key, 37);
        assert!(self.published.get());
        assert!(self.pending.get().is_none());
        let removed = self.question.borrow_mut().take();
        drop(removed);
    }
    fn idle(&self) {
        assert!(self.question.try_borrow_mut().is_ok());
    }
}
struct Lease<'s, 'a> {
    last: Last<'s, Context<'a>>,
}
impl<'a> Deref for Lease<'_, 'a> {
    type Target = Context<'a>;
    fn deref(&self) -> &Self::Target {
        self.last.owner()
    }
}
fn exercise() {
    let mut words = [ZERO; 8];
    let question = Question::new();
    let bytes = [17u8; 32];
    let context = Context {
        question: RefCell::new(Some(question)),
        record: RefCell::new(Reference::new(Some(7), 37, Some(()))),
        pending: Cell::new(None),
        output: RefCell::new(message::FixedBuilder::new(&mut words).unwrap()),
        capacity: Cell::new(false),
        published: Cell::new(false),
    };
    assert_eq!(context.record.borrow().context(), Some(&7));
    assert!(context.record.borrow_mut().replace_pipeline(11).is_none());
    assert_eq!(context.record.borrow().pipeline(), Some(&11));
    let mut receiver = direct::Receiver::new(Lease {
        last: Last::new(&context, 37),
    });
    let mut cx = TaskContext::from_waker(Waker::noop());
    assert!(Pin::new(&mut receiver).poll(&mut cx).is_pending());
    context
        .question
        .borrow_mut()
        .as_mut()
        .unwrap()
        .returned(false)
        .unwrap();
    assert_eq!(context.record.borrow_mut().take_producer(), Some(()));
    assert!(direct::deliver(&context, core::future::ready(&bytes[..])).is_none());
    let Poll::Ready(response) = Pin::new(&mut receiver).poll(&mut cx) else {
        panic!("response not ready")
    };
    drop(receiver);
    assert!(context.question.borrow().is_some());
    assert!(!context.published.get());
    context.capacity.set(true);
    let (finish, reclaim) = context.pending.take().unwrap();
    release::resume(&context, &37, finish, reclaim);
    assert!(context.question.borrow().is_none());
    assert_eq!(response, &[17u8; 32]);
    let output = context.output.borrow();
    let root = output
        .get_root_as_reader::<rpc_capnp::message::Reader<'_>>()
        .unwrap();
    let rpc_capnp::message::Finish(finish) = root.which().unwrap() else {
        panic!("Finish lost")
    };
    let finish = finish.unwrap();
    assert_eq!(finish.get_question_id(), 37);
    assert!(!finish.get_release_result_caps());
}
#[cfg(not(target_os = "none"))]
fn main() {
    exercise();
}
#[cfg(target_os = "none")]
#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    exercise();
    loop {
        core::hint::spin_loop();
    }
}
#[cfg(target_os = "none")]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
