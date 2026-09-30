use capnp::{
    capability::Promise,
    message::{Builder, HeapAllocator},
    Error,
};
use capnp_rpc::{FlowController, OutgoingMessage};
use futures::{channel::oneshot, FutureExt};
use std::{cell::Cell, rc::Rc};
struct Message {
    words: usize,
    sent: Rc<Cell<u32>>,
    body: Builder<HeapAllocator>,
}
impl OutgoingMessage for Message {
    fn get_body(&mut self) -> capnp::Result<capnp::any_pointer::Builder<'_>> {
        self.body.get_root()
    }
    fn get_body_as_reader(&self) -> capnp::Result<capnp::any_pointer::Reader<'_>> {
        self.body.get_root_as_reader()
    }
    fn size_in_words(&self) -> usize {
        self.words
    }
    fn take(self: Box<Self>) -> Builder<HeapAllocator> {
        self.body
    }
    fn send(self: Box<Self>) -> (Promise<(), Error>, Rc<Builder<HeapAllocator>>) {
        self.sent.set(self.sent.get() + 1);
        (Promise::ok(()), Rc::new(self.body))
    }
}
struct Fixture {
    controller: Option<Box<dyn FlowController>>,
    driver: tokio::task::JoinHandle<capnp::Result<()>>,
    sent: Rc<Cell<u32>>,
    acks: Vec<Option<oneshot::Sender<capnp::Result<()>>>>,
    promises: Vec<Option<Promise<(), Error>>>,
    outcomes: [u32; 2],
    drain: Option<Promise<(), Error>>,
    drained: bool,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.driver.abort();
    }
}
impl Fixture {
    fn new(window: usize) -> Self {
        let (controller, driver) = capnp_rpc::new_fixed_window_flow_controller(window * 8);
        Self {
            controller: Some(controller),
            driver: tokio::task::spawn_local(driver),
            sent: Rc::new(Cell::new(0)),
            acks: vec![],
            promises: vec![],
            outcomes: [0; 2],
            drain: None,
            drained: false,
        }
    }
    async fn replay(&mut self, steps: &[serde_json::Value]) {
        for step in steps {
            let action = step["action"].as_str().unwrap();
            match action {
                "send" => {
                    let (tx, rx) = oneshot::channel();
                    let message = Box::new(Message {
                        words: self.acks.len() + 1,
                        sent: self.sent.clone(),
                        body: Builder::new_default(),
                    });
                    let ack = Promise::from_future(async move { rx.await.unwrap() });
                    self.promises
                        .push(Some(self.controller.as_mut().unwrap().send(message, ack)));
                    self.acks.push(Some(tx));
                }
                "ack_first" | "fail_first" | "ack_second" | "fail_second" => {
                    let i = usize::from(action.ends_with("second"));
                    self.acks[i]
                        .take()
                        .unwrap()
                        .send(if action.starts_with("fail") {
                            Err(Error::disconnected("stream failed".into()))
                        } else {
                            Ok(())
                        })
                        .unwrap();
                }
                "wait" => {
                    self.drain = Some(self.controller.as_mut().unwrap().wait_all_acked());
                }
                "drop" => {
                    self.controller.take();
                }
                _ => panic!("{action}"),
            }
            for _ in 0..8 {
                tokio::task::yield_now().await;
            }
            for (i, promise) in self.promises.iter_mut().enumerate() {
                if let Some(p) = promise {
                    if let Some(result) = p.now_or_never() {
                        self.outcomes[i] = if result.is_ok() { 1 } else { 3 };
                        *promise = None;
                    } else {
                        self.outcomes[i] = 2;
                    }
                }
            }
            if let Some(p) = self.drain.as_mut() {
                if let Some(result) = p.now_or_never() {
                    result.unwrap();
                    self.drained = true;
                    self.drain = None;
                }
            }
            let state: Vec<u32> = serde_json::from_value(step["state"].clone()).unwrap();
            assert_eq!(
                self.sent.get(),
                state[0],
                "send must happen immediately at {step}"
            );
            assert_eq!(self.outcomes, [state[3], state[4]], "credit at {step}");
            assert_eq!(self.drained, state[8] != 0, "drain at {step}");
            let all_acked = (state[0] == 0 || state[1] != 0) && (state[0] < 2 || state[2] != 0);
            assert_eq!(
                self.driver.is_finished(),
                state[6] != 0 && all_acked,
                "driver lifetime at {step}"
            );
        }
    }
}
#[tokio::test(flavor = "current_thread")]
async fn replay_tlc_streaming_traces() {
    let path = reproto_test_support::verification::input("REPROTO_STREAMING_TRACES")
        .expect("prepare verified trace corpus");
    let cases: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    tokio::task::LocalSet::new()
        .run_until(async {
            for case in cases {
                Fixture::new(case["window"].as_u64().unwrap() as usize)
                    .replay(case["steps"].as_array().unwrap())
                    .await;
            }
        })
        .await;
}
#[tokio::test(flavor = "current_thread")]
async fn empty_stream_drains_and_driver_terminates() {
    tokio::task::LocalSet::new()
        .run_until(async {
            let (mut controller, driver) = capnp_rpc::new_fixed_window_flow_controller(0);
            let task = tokio::task::spawn_local(driver);
            tokio::time::timeout(
                std::time::Duration::from_secs(1),
                controller.wait_all_acked(),
            )
            .await
            .unwrap()
            .unwrap();
            drop(controller);
            tokio::time::timeout(std::time::Duration::from_secs(1), task)
                .await
                .unwrap()
                .unwrap()
                .unwrap();
        })
        .await;
}
