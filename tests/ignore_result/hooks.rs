use capnp::{
    any_pointer,
    capability::{Promise, RemotePromise, Request, Response},
    private::capability::{ClientHook, PipelineHook, PipelineOp, RequestHook, ResponseHook},
    Error,
};
use futures::{channel::oneshot, FutureExt};
use std::{cell::Cell, marker::PhantomData, rc::Rc};

#[derive(Default)]
struct Counts {
    sends: Cell<u32>,
    pipelines: Cell<u32>,
    responses: Cell<u32>,
}
struct ResponseGuard(Rc<Counts>);
impl Drop for ResponseGuard {
    fn drop(&mut self) {
        self.0.responses.set(self.0.responses.get() + 1);
    }
}
impl ResponseHook for ResponseGuard {
    fn get(&self) -> capnp::Result<any_pointer::Reader<'_>> {
        panic!("ignored results must never be decoded")
    }
}
struct PipelineGuard(Rc<Counts>);
impl Drop for PipelineGuard {
    fn drop(&mut self) {
        self.0.pipelines.set(self.0.pipelines.get() + 1);
    }
}
impl PipelineHook for PipelineGuard {
    fn add_ref(&self) -> Box<dyn PipelineHook> {
        panic!("unused pipeline cloned")
    }
    fn get_pipelined_cap(&self, _: &[PipelineOp]) -> Box<dyn ClientHook> {
        panic!("unused pipeline projected")
    }
}
struct Hook {
    counts: Rc<Counts>,
    gate: oneshot::Receiver<capnp::Result<()>>,
}
impl RequestHook for Hook {
    fn get(&mut self) -> any_pointer::Builder<'_> {
        panic!("parameters untouched")
    }
    fn get_brand(&self) -> usize {
        0
    }
    fn send(self: Box<Self>) -> RemotePromise<any_pointer::Owned> {
        let Hook { counts, gate } = *self;
        counts.sends.set(counts.sends.get() + 1);
        let response = Response {
            hook: Box::new(ResponseGuard(counts.clone())),
            marker: PhantomData,
        };
        RemotePromise {
            promise: Promise::from_future(async move {
                gate.await.unwrap()?;
                Ok(response)
            }),
            pipeline: any_pointer::Pipeline::new(Box::new(PipelineGuard(counts))),
        }
    }
    fn send_streaming(self: Box<Self>) -> Promise<(), Error> {
        panic!("not a streaming call")
    }
    fn tail_send(self: Box<Self>) -> Option<(u32, Promise<(), Error>, Box<dyn PipelineHook>)> {
        panic!("not a tail call")
    }
}

#[tokio::test(flavor = "current_thread")]
async fn ignores_schema_and_pipeline_but_retains_completion_and_exact_error() {
    for mode in 0..4 {
        let counts = Rc::new(Counts::default());
        let (tx, rx) = oneshot::channel();
        // Unit implements neither Owned nor Pipelined: ignored result data has
        // no type constraints and is never accessed, even via ResponseHook.
        let request = Request::<(), ()> {
            marker: PhantomData,
            hook: Box::new(Hook {
                counts: counts.clone(),
                gate: rx,
            }),
        };
        let mut promise = request.send_ignoring_result();
        assert_eq!(counts.sends.get(), 1, "send happens before polling");
        assert_eq!(
            counts.pipelines.get(),
            1,
            "pipeline released before polling"
        );
        assert_eq!(counts.responses.get(), 0);
        if mode != 3 {
            assert!((&mut promise).now_or_never().is_none());
        }
        match mode {
            0 => {
                tx.send(Ok(())).unwrap();
                promise.await.unwrap();
            }
            1 => {
                tx.send(Err(Error::overloaded("remote overloaded".into())))
                    .unwrap();
                let error = promise.await.unwrap_err();
                assert_eq!(error.kind, capnp::ErrorKind::Overloaded);
                assert_eq!(error.extra, "remote overloaded");
            }
            _ => {
                drop(promise);
                assert!(tx.is_canceled());
            }
        }
        assert_eq!(counts.responses.get(), 1);
        assert_eq!(counts.sends.get(), 1);
        assert_eq!(counts.pipelines.get(), 1);
    }
}
