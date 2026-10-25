//! Own the generative protocol/message scopes inside one ordinary 'static task.
use super::*;
use crate::scoped::{
    wire::{resources::Import, server::Producer},
    ClientHooks,
};
use alloc::{boxed::Box, string::ToString, vec::Vec};
use capnp::private::capability::ClientHook;
use capntproto_rpc_engine::{connection as storage, messages};
use capntproto_rpc_wire::owned::connection as wire;

pub(super) fn connect<V: 'static>(
    connections: &Rc<Connections<State>>,
    context: &Rc<ContextOwner<V>>,
    connection: Box<dyn crate::Connection<V>>,
) -> capnp::Result<Rc<State>> {
    let id = connection.connection_id();
    let registration = match connections.acquire_bounded(id, context.options.connections.get())? {
        Acquired::Existing(state) => return Ok(state),
        Acquired::New(registration) => registration,
    };
    let commands = Rc::new(coordinator::Commands::new(context.options.concurrency));
    let (stop, stopped) = shared::channel();
    let (abort, aborted) = shared::channel();
    // Capture peer callbacks only after reserving the trusted connection ID.
    let connection = vat::Connection::with_identity(connection, id);
    let idle = connection.idle_notifier();
    let network_commands = match connection.network_connection().borrow().control_mode() {
        crate::ControlMode::TwoParty => None,
        crate::ControlMode::Network => {
            let commands = coordinator::multiparty::Commands::new(
                context.options.concurrency.get(),
                context.options.message_bytes.get(),
            );
            *commands.network.borrow_mut() = Some(Rc::new(super::introduction::Network {
                connection: connection.network_connection(),
                connections: Rc::downgrade(connections),
                context: Rc::downgrade(context),
            }));
            Some(commands)
        }
    };
    let state = Rc::new(State {
        id,
        handle: Handle::new(&commands),
        stop: RefCell::new(Some(stop)),
        abort: RefCell::new(Some(abort)),
        policy: coordinator::policy::Policy::with_settings(context.policy.clone()),
        idle,
        network_controls: network_commands
            .as_ref()
            .map_or_else(coordinator::multiparty::Handle::unavailable, |commands| {
                commands.handle()
            }),
    });
    // Capture the transport peer only after reserving its identity. Custom
    // callbacks can reenter; the reserved identity cannot create a second driver.
    let published = registration.publish(state.clone());
    let retained = state.clone();
    let bootstrap = context.bootstrap.clone();
    let options = context.options;
    let executor = Rc::new(crate::CallTaskExecutor(context.tasks.clone()));
    let policy = state.policy.clone();
    let idle = state.idle.clone();
    let joins = context.join_network.clone();
    context
        .tasks
        .try_add_connection(async move {
            let outcome = run(
                connection,
                bootstrap,
                options,
                commands,
                executor,
                stopped,
                aborted,
                policy,
                idle,
                network_commands,
                joins,
            )
            .await;
            retained.close_clients(outcome.as_ref().err().cloned().unwrap_or_else(closing));
            registration.detach();
            registration.complete(outcome.clone());
            outcome
        })
        .map_err(|_| closing())?;
    if let Err(error) = published {
        state.abort(error.clone());
        return Err(error);
    }
    Ok(state)
}

#[allow(clippy::too_many_arguments)]
async fn run<V: 'static>(
    connection: vat::Connection<V>,
    bootstrap: Bootstrap<V>,
    options: Options,
    commands: Rc<coordinator::Commands>,
    executor: Rc<dyn capability::CallExecutor>,
    stopped: shared::Receiver<capnp::Result<()>>,
    aborted: shared::Receiver<Error>,
    policy: Rc<coordinator::policy::Policy>,
    idle: Rc<dyn Fn(bool)>,
    network_commands: Option<Rc<coordinator::multiparty::Commands>>,
    joins: Option<Rc<dyn crate::multiparty::JoinNetwork<V>>>,
) -> capnp::Result<()> {
    let mut buffers = messages::GrowingBuffers::new(
        options.message_buffers.get(),
        options.message_bytes.get(),
        options.retained_bytes.get(),
    );
    let bound = storage::Growth {
        initial: 0,
        maximum: options.table_entries.get(),
    };
    let network_connection = connection.network_connection();
    let (peer, mut reader, mut writer, failure) =
        connection.into_io_and_peer(options.message_bytes)?;
    let peer = &peer;
    let mut scratch = Vec::new();
    scratch
        .try_reserve_exact(options.message_capabilities)
        .map_err(error)?;
    scratch.resize(options.message_capabilities, 0);
    messages::with_growing_messages_async(
        &mut buffers,
        storage::Growth {
            initial: 0,
            maximum: options.output_queue.get(),
        },
        async |messages| {
            storage::with_domain_async(async |domain| {
                let graph = domain
                    .bind_growing_adoptions(
                        storage::GrowingStorage {
                            notifications:
                                capntproto_rpc_engine::clients::NotificationGrowth::for_clients(
                                    bound,
                                ),
                            imports: bound,
                            exports: bound,
                            answers: bound,
                            questions: bound,
                            clients: bound,
                        },
                        storage::Growth {
                            initial: 0,
                            maximum: options.adopted_answers,
                        },
                    )
                    .map_err(error)?;
                // Const sizes are inline optimizations only; all per-message
                // maxima come from Options and grow through the same engine.
                // Keep the common one-field promised-answer transform inline.
                // Longer paths still grow, and zero limits still reject it.
                let mut driver: wire::Driver<
                    '_,
                    '_,
                    '_,
                    Import,
                    Producer,
                    (),
                    Rc<dyn ClientHook>,
                    0,
                    1,
                > = wire::Driver::new_growing(
                    graph,
                    wire::GrowingStorage::uniform(0, options.table_entries.get())
                        .with_messages(options.message_capabilities, options.pipeline_path),
                    (),
                )
                .map_err(|(e, _)| error(e))?;
                let [questions, answers, imports, exports] = driver.observe_counts();
                *policy.tables.borrow_mut() = Some(coordinator::policy::Tables {
                    questions,
                    answers,
                    imports,
                    exports,
                    embargoes: driver.observe_embargoes(),
                });
                driver.set_descriptors(coordinator::multiparty::introduction::descriptors());
                driver.use_owned_output_queues(true);
                let hooks = ClientHooks::new(options.hooks.get(), options.concurrency.get())
                    .with_admission(policy.admission.clone());
                #[cfg(feature = "std")]
                let hooks = hooks.with_message_pool(crate::MessagePool::new(
                    options.cached_message_words,
                    options.cached_message_segments,
                ));
                let mut hooks = hooks;
                if let Some(commands) = &network_commands {
                    hooks = hooks.with_joins(commands.handle());
                }
                let mut coordinator = coordinator::Coordinator::new_using_commands(
                    driver,
                    hooks,
                    options.concurrency,
                    Some(executor),
                    coordinator::Options {
                        input_bytes: options.message_bytes,
                        output_bytes: NonZeroUsize::new(128).unwrap(),
                        budget: options.poll_budget,
                        reader: options.reader,
                    },
                    commands,
                )?
                .with_policy(policy);
                if let Some(commands) = &network_commands {
                    coordinator.foreign = coordinator.foreign.with_introductions(
                        coordinator::multiparty::introduction::policy(commands.handle()),
                    );
                }
                let controls = coordinator::TwoParty::new(
                    || bootstrap.for_peer(peer).map(capability::Client::new),
                    options.concurrency,
                )
                .with_idle_notifier(move |value| idle(value));
                let mut queue = writer
                    .publication_queue(messages, options.output_queue.get())
                    .map_err(error)?;
                let mut aborted = core::pin::pin!(aborted);
                let mut failure = core::pin::pin!(failure);
                let abort = core::future::poll_fn(|cx| {
                    match aborted.as_mut().poll(cx) {
                        Poll::Ready(result) => {
                            return Poll::Ready(result.unwrap_or_else(|_| closing()))
                        }
                        Poll::Pending => (),
                    }
                    failure.as_mut().poll(cx)
                });
                // Select the handler once. The ordinary stream loop has no
                // multiparty collections, weak command queues or feature probes.
                if let Some(network_commands) = network_commands {
                    let mut controls = coordinator::multiparty::NetworkControl::new(
                        controls,
                        network_connection,
                        joins,
                        network_commands,
                        options.concurrency.get(),
                    );
                    coordinator
                        .run_connection(
                            messages,
                            &mut queue,
                            &mut reader,
                            &mut writer,
                            &mut scratch,
                            &vat::Resources::new(options.maximum_fds),
                            &mut controls,
                            async { stopped.await.unwrap_or_else(|_| Err(closing())) },
                            abort,
                        )
                        .await
                        .map_err(run_error)
                } else {
                    let mut controls = controls;
                    let result = coordinator
                        .run_connection(
                            messages,
                            &mut queue,
                            &mut reader,
                            &mut writer,
                            &mut scratch,
                            &vat::Resources::new(options.maximum_fds),
                            &mut controls,
                            async { stopped.await.unwrap_or_else(|_| Err(closing())) },
                            abort,
                        )
                        .await;
                    // The idle notifier is weak. Retain this transport owner
                    // through drain even though no NetworkControl owns it.
                    drop(network_connection);
                    result.map_err(run_error)
                }
            })
            .await
        },
    )
    .await
    .map_err(error)?
}
fn error(error: impl core::fmt::Display) -> Error {
    Error::failed(error.to_string())
}
fn run_error(error: coordinator::RunError<Error, Error>) -> Error {
    use capntproto_rpc_engine::io::SendError;
    use capntproto_rpc_wire::{io::ReceiveError, owned::tasks};
    match error.cause {
        Some(
            coordinator::Cause::Protocol(error)
            | coordinator::Cause::Close(error)
            | coordinator::Cause::Output(SendError::Io(error))
            | coordinator::Cause::Input(tasks::ReceiveError::Input(ReceiveError::Io(error))),
        ) => error,
        other => Error::failed(format!(
            "portable RPC stopped: {other:?}; cleanup: {:?}",
            error.cleanup
        )),
    }
}
