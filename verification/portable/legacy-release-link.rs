//! Deferred legacy Finish publication from a borrowed last-interest finalizer.
#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]
use capnp::message;
use capntproto_rpc_engine::question::{
    release::{self, Last, Owner, Publication},
    Finish, Question, Release,
};
use capntproto_rpc_wire::rpc_capnp::{self, api};
use core::cell::{Cell, RefCell};
const ZERO: capnp::Word = capnp::word(0, 0, 0, 0, 0, 0, 0, 0);
struct Context<'a> {
    question: RefCell<Option<Question>>,
    pending: Cell<Option<(Finish, bool)>>,
    output: RefCell<message::FixedBuilder<'a>>,
    capacity: Cell<bool>,
    published: Cell<bool>,
}
impl Owner for Context<'_> {
    type Key = u32;
    fn release(&self, key: &u32) -> Option<Release> {
        assert_eq!(*key, 37);
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
fn exercise() {
    let mut words = [ZERO; 8];
    let mut question = Question::new();
    question.returned(false).unwrap();
    let context = Context {
        question: RefCell::new(Some(question)),
        pending: Cell::new(None),
        output: RefCell::new(message::FixedBuilder::new(&mut words).unwrap()),
        capacity: Cell::new(false),
        published: Cell::new(false),
    };
    drop(Last::new(&context, 37));
    assert!(context.question.borrow().is_some());
    assert!(!context.published.get());
    context.capacity.set(true);
    let (finish, reclaim) = context.pending.take().unwrap();
    release::resume(&context, &37, finish, reclaim);
    assert!(context.question.borrow().is_none());
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
