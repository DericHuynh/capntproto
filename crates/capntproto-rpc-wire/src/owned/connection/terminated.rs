//! Explicit terminal cleanup while retaining the shared message-pool scope.
use super::*;

type ClosedLedgers<'p, 'id, 's, I, A, const CAPS: usize> =
    connection::Terminated<'id, 's, I, clients::Client<'id>, Answer<'p, 'id, A, CAPS>>;
type ClosedRuntime<'p, 'id, 's, I, P, S, const CAPS: usize, const PATH: usize> =
    dispatch::Terminated<
        'id,
        's,
        's,
        binding::Route<'id, P, S, PATH>,
        P,
        Task<'p, 'id, I, CAPS, PATH>,
    >;

// Each phase owns its state inline; boxing the larger active phase would add
// the allocator this connection deliberately avoids.
#[allow(clippy::large_enum_variant)]
enum Stage<'p, 'id, 's, I, A, P: Copy + Eq, S, const CAPS: usize, const PATH: usize> {
    Tasks(Driver<'p, 'id, 's, I, A, P, S, CAPS, PATH>),
    Ledgers {
        ledgers: ClosedLedgers<'p, 'id, 's, I, A, CAPS>,
        runtime: Runtime<'p, 'id, 's, I, P, S, CAPS, PATH>,
    },
    Endpoints(ClosedRuntime<'p, 'id, 's, I, P, S, CAPS, PATH>),
    Done,
}
pub enum Termination<I, A, S> {
    Pending,
    Done,
    Import(I),
    Producer(A),
    Server(S),
}

/// A terminal driver has no input, dispatch or publication methods. Stop input
/// and return externally executing tasks/response frames before consuming it.
/// Serialized publications remain owned by their output queue: drain or discard
/// them separately. Poll protocol cleanup with the original message pool. Owners
/// are returned outside ledger/graph borrows; no application callback runs here.
/// Finish polling, or end the message-pool scope if this value is dropped early.
#[must_use = "poll terminal cleanup or end the enclosing message-pool scope"]
pub struct Terminating<'p, 'id, 's, I, A, P: Copy + Eq, S, const CAPS: usize, const PATH: usize> {
    stage: Stage<'p, 'id, 's, I, A, P, S, CAPS, PATH>,
    phase: usize,
    index: usize,
}
impl<'p, 'id, 's, I, A, P: Copy + Eq, S, const CAPS: usize, const PATH: usize>
    Driver<'p, 'id, 's, I, A, P, S, CAPS, PATH>
{
    pub fn terminate(self) -> Terminating<'p, 'id, 's, I, A, P, S, CAPS, PATH> {
        Terminating {
            stage: Stage::Tasks(self),
            phase: 1,
            index: 0,
        }
    }
}
impl<'p, I, A, P: Copy + Eq, S, const CAPS: usize, const PATH: usize>
    Terminating<'p, '_, '_, I, A, P, S, CAPS, PATH>
{
    pub fn poll(
        &mut self,
        messages: &mut messages::Messages<'p, '_, '_>,
        budget: core::num::NonZeroUsize,
    ) -> Result<Termination<I, A, S>, Error> {
        for _ in 0..budget.get() {
            if let Some(owner) = self.step(messages) {
                return Ok(owner);
            }
        }
        Ok(Termination::Pending)
    }
    fn step(
        &mut self,
        messages: &mut messages::Messages<'p, '_, '_>,
    ) -> Option<Termination<I, A, S>> {
        match &mut self.stage {
            Stage::Tasks(driver) => match self.phase {
                1 => {
                    if let Some(request) = driver.network.take() {
                        request.release(driver, messages);
                    } else if let Some(call) = driver.call.take() {
                        driver.clear_call_target();
                        driver.release_call(call, messages);
                    } else {
                        self.phase += 1;
                    }
                }
                2 => {
                    if let Some(echo) = driver.echo.take() {
                        echo.release(&mut driver.ledgers.exports, &mut driver.ledgers.answers);
                    } else {
                        self.phase += 1;
                    }
                }
                3 => {
                    if let Some(task) = driver.runtime.drain_one() {
                        match task {
                            Task::Call(call) => driver.release_call(call, messages),
                            Task::Invoke(call) => call.release(messages, &mut driver.runtime),
                            Task::Echo(echo) => echo
                                .release(&mut driver.ledgers.exports, &mut driver.ledgers.answers),
                            Task::Loopback(_) => (),
                            Task::ProvisionBarrier(barrier) => barrier.release(messages),
                            Task::VineBarrier(barrier) => {
                                barrier.release(messages, &mut driver.ledgers.exports)
                            }
                        }
                    } else {
                        self.phase += 1;
                    }
                }
                4 => {
                    if let Some(resolution) = driver.resolution.take() {
                        resolution.release(
                            messages,
                            &mut driver.ledgers.imports,
                            &mut driver.ledgers.exports,
                            &mut driver.ledgers.answers,
                        );
                    } else {
                        self.phase += 1;
                    }
                }
                5 => {
                    if self.index < driver.exchanges.len() {
                        let exchange = driver.exchanges[self.index].exchange.take();
                        if let Some(Ok(result)) = driver.exchanges[self.index].handoff.take() {
                            result.release(messages, &mut driver.runtime);
                        }
                        driver.exchanges[self.index].canceled = false;
                        self.index += 1;
                        match exchange {
                            Some(Exchange::Pending(question)) => {
                                driver.clear_exchange_work(question.key());
                                driver.ledgers.questions.release_owner(question.key());
                                question.abandon(messages, &mut driver.ledgers.questions)
                            }
                            Some(Exchange::Returned(response)) => {
                                driver.release_response(response, messages)
                            }
                            None => (),
                        }
                    } else {
                        self.phase += 1;
                        self.index = 0;
                    }
                }
                6 => {
                    if !driver.exported.drain_one(&mut driver.runtime) {
                        self.phase += 1;
                    }
                }
                7 => {
                    if !driver
                        .pipelines
                        .drain_one(&mut driver.ledgers.answers, &mut driver.runtime)
                    {
                        self.phase += 1;
                    }
                }
                8 => {
                    if driver.loopbacks.drain_one().is_none() {
                        self.phase += 1;
                    }
                }
                9 => {
                    driver.outgoing.clear_terminal(&mut driver.runtime);
                    driver
                        .invocation_pipelines
                        .clear_terminal(&mut driver.runtime);
                    let Stage::Tasks(driver) = core::mem::replace(&mut self.stage, Stage::Done)
                    else {
                        unreachable!()
                    };
                    let Driver {
                        ledgers, runtime, ..
                    } = driver;
                    self.stage = Stage::Ledgers {
                        ledgers: ledgers.terminate(),
                        runtime,
                    };
                    self.phase = 10;
                }
                _ => unreachable!(),
            },
            Stage::Ledgers { ledgers, runtime } => {
                match self.phase {
                    10 => {
                        if self.index < ledgers.answer_capacity() {
                            let answer = ledgers.take_answer(self.index);
                            self.index += 1;
                            if let Some(answer) = answer {
                                return Some(Termination::Producer(
                                    answer.release(messages).release(messages, runtime, |_| {}),
                                ));
                            }
                        } else {
                            self.index = 0;
                            self.phase += 1;
                        }
                    }
                    11 => {
                        if self.index < ledgers.export_capacity() {
                            let client = ledgers.take_export(self.index);
                            self.index += 1;
                            if let Some(client) = client {
                                runtime.release(client);
                            }
                        } else {
                            self.index = 0;
                            self.phase += 1;
                        }
                    }
                    12 => {
                        if self.index < ledgers.import_capacity() {
                            let owner = ledgers.take_import(self.index);
                            self.index += 1;
                            if let Some(owner) = owner {
                                return Some(Termination::Import(owner));
                            }
                        } else {
                            let Stage::Ledgers { runtime, .. } =
                                core::mem::replace(&mut self.stage, Stage::Done)
                            else {
                                unreachable!()
                            };
                            self.stage = Stage::Endpoints(runtime.terminate());
                            self.index = 0;
                            self.phase = 13;
                        }
                    }
                    _ => unreachable!(),
                }
            }
            Stage::Endpoints(runtime) => {
                if self.index < runtime.endpoint_capacity() {
                    let route = runtime
                        .take_endpoint(self.index)
                        .expect("tasks drained before graph teardown");
                    self.index += 1;
                    if let Some(
                        binding::Route::Capability(binding::Endpoint::Local(server))
                        | binding::Route::Capability(binding::Endpoint::Failed {
                            owner: server, ..
                        })
                        | binding::Route::Vine(server),
                    ) = route
                    {
                        return Some(Termination::Server(server));
                    }
                } else {
                    self.stage = Stage::Done;
                    return Some(Termination::Done);
                }
            }
            Stage::Done => return Some(Termination::Done),
        }
        None
    }
}
