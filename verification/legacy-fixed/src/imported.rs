//! Borrowed import cleanup owned by the actual legacy parameter adapter.
use super::*;
use capntproto_rpc_engine::references::{import, ImportRefs};

struct Connection<'a> {
    input_drops: &'a Cell<usize>,
    started: &'a Cell<usize>,
    retired: &'a Cell<usize>,
    active: Cell<bool>,
    pending: Cell<u32>,
    idle: Cell<usize>,
}
impl import::Owner for Connection<'_> {
    type Key = u32;
    fn retire(&self, key: &u32) {
        assert_eq!(*key, 7);
        assert_eq!(self.input_drops.get(), 1, "input must drop before bindings");
        assert_eq!(self.started.get(), self.retired.get());
        assert!(self.active.replace(false));
    }
    fn release(&self, _: &u32, references: &mut ImportRefs) {
        assert!(!self.active.get());
        let ticket = references.pending_release().unwrap();
        // Output is deliberately full. The connection keeps the exact Release
        // after the last import and its suspended task have both disappeared.
        assert_eq!(self.pending.replace(ticket.count()), 0);
        ticket.commit();
    }
    fn idle(&self) {
        assert_eq!(self.pending.get(), 9);
        self.idle.set(self.idle.get() + 1);
    }
}
struct Bindings<'a>(import::Last<'a, Connection<'a>>);
impl capnp_rpc::incoming::Capabilities for Bindings<'_> {
    fn bind<'a>(&'a self, content: any_pointer::Reader<'a>) -> any_pointer::Reader<'a> {
        // This scalar method does not decode a capability. Retaining an import
        // is not itself a callable capability binding or descriptor admission.
        assert!(self.0.owner().active.get());
        content
    }
}

pub fn imported_generated_invocations() -> Result<()> {
    static WAKES: AtomicUsize = AtomicUsize::new(0);
    static NOTICE: Static<Notice> = Static::new(Notice(&WAKES));
    for mode in 0..4 {
        let mut words = [ZERO; 32];
        let input = source(&mut words, false)?;
        let mut output_words = [ZERO; 32];
        let mut output = message::FixedBuilder::new(&mut output_words)?;
        let input_drops = Cell::new(0);
        let retired = Cell::new(0);
        let started = Cell::new(0);
        let connection = Connection {
            input_drops: &input_drops,
            started: &started,
            retired: &retired,
            active: Cell::new(true),
            pending: Cell::new(0),
            idle: Cell::new(0),
        };
        let mut references = ImportRefs::new();
        references.receive(9).unwrap();
        let params = capnp_rpc::incoming::Params::new(
            Input {
                reader: message::Reader::new(&input, Default::default()),
                drops: &input_drops,
            },
            Bindings(import::Last::new(&connection, 7, references)),
        );
        let root = output.try_init_root::<api::Message>()?;
        let mut returned = root.into_field(api::Message::RETURN).init()?;
        returned.scalar(api::Return::ANSWER_ID).set(37);
        let content = returned
            .into_field(api::Return::RESULTS)
            .init()?
            .into_field(api::Payload::CONTENT);
        let invocation = CallContext::from_owner(PreparedCall::new(params, content));
        let mut gate = completion::Slot::new();
        let (ready, wait) = gate.pair().unwrap();
        let mut service = node::ServerDispatch::new(Server {
            gate: Some(wait),
            params: &input_drops,
            retired: &retired,
            started: &started,
        });
        let mut places = core::pin::pin!(tasks::PinnedSlots::<_, 1>::new());
        let [place] = places.as_mut().places();
        let mut task = Promise::from_owner(
            place
                .insert_erased(dispatch_call(
                    invocation,
                    &mut service,
                    if mode == 3 {
                        0
                    } else {
                        base::Owned::READ.interface_id()
                    },
                    0,
                ))
                .ok()
                .unwrap(),
        );
        let waker = NOTICE.waker();
        let mut cx = Context::from_waker(&waker);
        if mode == 3 {
            assert!(matches!(
                Pin::new(&mut task).poll(&mut cx),
                Poll::Ready(Err(Error {
                    kind: ErrorKind::Unimplemented,
                    ..
                }))
            ));
        } else if mode != 2 {
            assert!(Pin::new(&mut task).poll(&mut cx).is_pending());
            assert!(connection.active.get());
            assert_eq!(connection.pending.get(), 0);
            assert_eq!(input_drops.get(), 0);
            if mode == 0 {
                assert!(ready.send(()).is_ok());
                assert!(matches!(
                    Pin::new(&mut task).poll(&mut cx),
                    Poll::Ready(Ok(()))
                ));
            }
        }
        drop(task);
        assert!(places.is_empty());
        assert_eq!(input_drops.get(), 1);
        assert!(!connection.active.get());
        assert_eq!(connection.idle.get(), 1);
        // A later output turn owns and publishes precisely these nine wire
        // references. No destroyed task/import must be revisited to recover it.
        assert_eq!(connection.pending.take(), 9);
    }
    Ok(())
}
