#[allow(dead_code)]
mod support;
use capnp::field_api::Message;
use capntproto_test_support::field_api_capnp::service;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
struct Echo {
    returned: Option<service::Client>,
    streams: Rc<Cell<u64>>,
}
impl service::Server for Echo {
    async fn echo(
        self: Rc<Self>,
        params: service::EchoParams,
        mut results: service::EchoResults,
    ) -> capnp::Result<()> {
        results
            .get()
            .into_api()
            .person()
            .copy_from(params.get()?.into_api().person()?)
    }
    async fn make(
        self: Rc<Self>,
        _: service::MakeParams,
        mut results: service::MakeResults,
    ) -> capnp::Result<()> {
        results
            .get()
            .into_api()
            .cap()
            .copy_from(self.returned.as_ref().unwrap().clone())
    }
    async fn stream(self: Rc<Self>, params: service::StreamParams) -> capnp::Result<()> {
        self.streams.set(params.get()?.into_api().person()?.id());
        Ok(())
    }
}
struct Drivers(Vec<tokio::task::JoinHandle<capnp::Result<()>>>);
impl Drop for Drivers {
    fn drop(&mut self) {
        for task in &self.0 {
            task.abort();
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn generated_calls_support_edit_await_pipeline_and_streaming() {
    tokio::task::LocalSet::new()
        .run_until(async {
            for wire in [false, true] {
                let streams = Rc::new(Cell::new(0));
                let (executor, driver) = capnp_rpc::new_call_executor();
                let mut drivers = Drivers(vec![tokio::task::spawn_local(driver)]);
                let child: service::Client = capnp_rpc::new_client_with_executor(
                    Echo {
                        returned: None,
                        streams: streams.clone(),
                    },
                    executor.clone(),
                );
                let server: service::Client = capnp_rpc::new_client_with_executor(
                    Echo {
                        returned: Some(child),
                        streams: streams.clone(),
                    },
                    executor,
                );
                let client: service::Client = if wire {
                    let hub = Rc::new(RefCell::new(support::Hub::default()));
                    let host = capnp_rpc::RpcSystem::new(
                        Box::new(support::Hub::network(&hub, 1)),
                        Some(server.client.clone()),
                    );
                    let mut caller =
                        capnp_rpc::RpcSystem::new(Box::new(support::Hub::network(&hub, 2)), None);
                    let cap = caller.bootstrap(1);
                    drivers.0.push(tokio::task::spawn_local(host));
                    drivers.0.push(tokio::task::spawn_local(caller));
                    cap
                } else {
                    server
                };
                let pending = client.make_call().send();
                // The returned capability is usable before the parent response is awaited.
                let pipelined = pending.pipeline.get_cap();
                let mut call = pipelined.echo_call();
                call.edit()
                    .person()
                    .ensure()
                    .unwrap()
                    .name()
                    .copy_from("pipelined facade")
                    .unwrap();
                let answer = call.send().await.unwrap();
                assert_eq!(
                    answer.read().unwrap().person().unwrap().name().unwrap(),
                    "pipelined facade"
                );
                let parent = pending.await.unwrap();
                let retained = parent.read().unwrap().cap().unwrap();
                drop(parent);
                let mut stream = retained.stream_call();
                stream.edit().person().ensure().unwrap().id().set(71);
                stream.send().await.unwrap();
                let mut native = Message::<
                    capntproto_test_support::field_api_capnp::api::service::EchoParams,
                >::new()
                .unwrap();
                native.edit().person().ensure().unwrap().id().set(99);
                let mut call = retained.echo_call();
                call.copy_from(native.read()).unwrap();
                let (response, _pipeline) = call.send().into_parts();
                assert_eq!(
                    response
                        .await
                        .unwrap()
                        .read()
                        .unwrap()
                        .person()
                        .unwrap()
                        .id(),
                    99
                );
                assert_eq!(streams.get(), 71);
                // The generated field-operation facade also supports completion
                // without decoding results or retaining their capability pipeline.
                client.make_call().send_ignoring_result().await.unwrap();
                // Pipeline-only construction preserves the backend's call hints.
                let pipeline = client.make_call().send_for_pipeline();
                let mut call = pipeline.get_cap().echo_call();
                call.edit().person().ensure().unwrap().id().set(37);
                assert_eq!(
                    call.send()
                        .await
                        .unwrap()
                        .read()
                        .unwrap()
                        .person()
                        .unwrap()
                        .id(),
                    37
                );
            }
        })
        .await;
}
