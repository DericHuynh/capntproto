//! Prepare admitted portable calls for existing generated server hooks.
//! The caller retains the original running/answer owner until execution and
//! reply publication finish. These owned parameters do not borrow the driver.
use super::*;
mod results;
pub use results::{Execution, Results};
pub(crate) use results::{Transfer, Work};
mod worker;
pub use worker::{Progress, Worker};

/// An owned server invocation. Its capability table holds exported graph
/// routes, never peer-supplied numeric IDs. Construction does not execute the
/// application; a failed preparation leaves the original request retryable.
#[must_use = "dispatch the prepared invocation or drop its owned parameters"]
pub struct Prepared {
    method: DynamicCall,
    params: Box<dyn ParamsHook>,
}
impl ParamsHook for Payload {
    fn get(&self) -> capnp::Result<any_pointer::Reader<'_>> {
        self.reader()
    }
}
impl ParamsHook for SharedPayload {
    fn get(&self) -> capnp::Result<any_pointer::Reader<'_>> {
        capnp::private::capability::ResponseHook::get(self)
    }
}
impl Payload {
    pub(super) fn into_params(self) -> Box<dyn ParamsHook> {
        match self.body {
            PayloadBody::Shared { message } if self.caps.is_empty() => {
                // Admission proved this immutable frame has no capabilities.
                // The existing allocation already owns its storage and reader;
                // wrapping the Payload again would add a box to every call.
                debug_assert!(self.caps.is_empty());
                message
            }
            _ => Box::new(self),
        }
    }
}
impl Prepared {
    pub fn method(&self) -> DynamicCall {
        self.method
    }
    pub fn params(&self) -> capnp::Result<any_pointer::Reader<'_>> {
        self.params.get()
    }
    /// Use the admitted destination and its original result context. In
    /// particular, the adapter does not replace the context's executor,
    /// cancellation guard, early pipeline, or tail-call implementation.
    /// No graph/table borrow survives into application code or its future.
    pub fn dispatch(
        self,
        destination: &dyn ClientHook,
        results: Box<dyn ResultsHook>,
    ) -> Promise<(), Error> {
        destination.call_with_hints(
            self.method.interface,
            self.method.method,
            self.params,
            results,
            CallHints {
                no_promise_pipelining: self.method.no_promise_pipelining,
                only_promise_pipeline: self.method.only_promise_pipeline,
            },
        )
    }
}

impl<'id> ClientHooks<'id> {
    /// Convert parameters of a locally executing invocation. The task remains
    /// owned by the caller and must be completed through its Invocations binding.
    pub fn prepare_local_server<
        'pool,
        I,
        A,
        P: Copy + Eq,
        S,
        const CAPS: usize,
        const PATH: usize,
    >(
        &mut self,
        call: &connection::LocalCall<'pool, 'id, I, CAPS, PATH>,
        driver: &mut Driver<'pool, 'id, '_, I, A, P, S, CAPS, PATH>,
        messages: &messages::Messages<'pool, '_, '_>,
    ) -> capnp::Result<Prepared> {
        call.params(messages, |params| {
            let caps = params
                .capabilities()
                .iter()
                .map(|cap| {
                    cap.as_ref()
                        .map(|cap| {
                            self.export_client(&mut driver.runtime, cap)
                                .map(|client| client.hook)
                        })
                        .transpose()
                })
                .collect::<capnp::Result<Vec<_>>>()?;
            let mut content = params.content()?;
            content.imbue(&caps);
            Ok(Prepared {
                method: params.hints(),
                params: Payload::copy(content)?.into_params(),
            })
        })
        .map_err(wire_error)?
    }

    /// Convert an admitted incoming Call, retaining each capability through its
    /// exact import/export/answer receipt. Raw descriptor IDs are not used for
    /// lookup. The original request and its answer lifecycle remain unchanged.
    pub fn prepare_received_server<
        'pool,
        I,
        A,
        P: Copy + Eq,
        S,
        const CAPS: usize,
        const PATH: usize,
    >(
        &mut self,
        call: &connection::Request<'pool, 'id, I, CAPS, PATH>,
        driver: &mut Driver<'pool, 'id, '_, I, A, P, S, CAPS, PATH>,
        messages: &messages::Messages<'pool, '_, '_>,
    ) -> capnp::Result<Prepared> {
        if let Some(message) = call.share_plain_params(messages).map_err(wire_error)? {
            return Ok(Prepared {
                method: call.method(),
                params: message,
            });
        }
        call.with(messages, |params| {
            let mut caps = Vec::with_capacity(params.capability_count());
            for index in 0..params.capability_count() {
                let hook = match params.capability_at(index as u32).map_err(wire_error)? {
                    None => None,
                    Some(cap) => {
                        let client = driver
                            .imports
                            .bind_capability(
                                cap,
                                &mut driver.runtime,
                                &mut driver.ledgers.imports,
                                &driver.ledgers.exports,
                                &mut driver.ledgers.answers,
                                &mut driver.pipelines,
                            )
                            .map_err(wire_error)?
                            .into_client();
                        let hook = self.export_client(&mut driver.runtime, &client);
                        driver.runtime.release(client);
                        Some(hook?.hook)
                    }
                };
                caps.push(hook);
            }
            Ok(Prepared {
                method: params.incoming().hints(),
                params: Payload::retain_admitted_or_copy(
                    params.incoming().payload()?.get_content(),
                    call.share_frame(messages),
                    caps,
                )?
                .into_params(),
            })
        })
        .map_err(wire_error)?
    }
}
