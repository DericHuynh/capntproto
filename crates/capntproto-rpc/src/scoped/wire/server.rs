//! Prepare admitted portable calls for existing generated server hooks.
//! The caller retains the original running/answer owner until execution and
//! reply publication finish. These owned parameters do not borrow the driver.
use super::*;
mod results;
pub use results::{Execution, PlainPayload, Producer, Results};
pub(crate) use results::{NativeTail, Transfer, Work};
mod worker;
pub use worker::{Progress, Worker};

/// An owned server invocation. Its capability table holds exported graph
/// routes, never peer-supplied numeric IDs. Construction does not execute the
/// application; a failed preparation leaves the original request retryable.
#[must_use = "dispatch the prepared invocation or drop its owned parameters"]
pub struct Prepared {
    method: DynamicCall,
    params: Parameters,
}
// Immediate dispatch borrows its parameters. Only a handler that takes
// ownership needs the public boxed hook; retaining the admitted reader does
// not require an allocation of its own.
enum Parameters {
    Plain(owned::PlainParams),
    Hook(Box<dyn ParamsHook>),
}
impl std::ops::Deref for Parameters {
    type Target = dyn ParamsHook;
    fn deref(&self) -> &Self::Target {
        match self {
            Self::Plain(params) => params,
            Self::Hook(params) => &**params,
        }
    }
}
impl Parameters {
    fn into_hook(self) -> Box<dyn ParamsHook> {
        match self {
            Self::Plain(params) => Box::new(params),
            Self::Hook(params) => params,
        }
    }
}
impl From<Box<dyn ParamsHook>> for Parameters {
    fn from(params: Box<dyn ParamsHook>) -> Self {
        Self::Hook(params)
    }
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
                // Move the reader directly into the parameter hook without
                // a second wrapper around the capability-free payload.
                debug_assert!(self.caps.is_empty());
                Box::new(message)
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
            self.params.into_hook(),
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
                            self.export_client(&mut driver.capabilities(), cap)
                                .map(|client| client.hook)
                        })
                        .transpose()
                })
                .collect::<capnp::Result<Vec<_>>>()?;
            let mut content = params.content()?;
            content.imbue(&caps);
            Ok(Prepared {
                method: params.hints(),
                params: Payload::copy(content)?.into_params().into(),
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
        self.prepare_received_server_using(call, driver, messages, |_| None)
    }

    pub(crate) fn prepare_received_server_using<
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
        mut local_hook: impl FnMut(&S) -> Option<Box<dyn ClientHook>>,
    ) -> capnp::Result<Prepared> {
        if let Some(message) = call.share_plain_params(messages).map_err(wire_error)? {
            return Ok(Prepared {
                method: call.method(),
                params: Parameters::Plain(message),
            });
        }
        self.collect(&mut driver.capabilities());
        call.with(messages, |params| {
            let mut caps = Vec::with_capacity(params.capability_count());
            for index in 0..params.capability_count() {
                let hook = match params.capability_at(index as u32).map_err(wire_error)? {
                    None => None,
                    Some(cap) => {
                        let client = driver
                            .bind_capability(cap)
                            .map_err(wire_error)?
                            .into_client();
                        Some(self.import_wire_capability(driver, client, &mut local_hook)?)
                    }
                };
                caps.push(hook);
            }
            Ok(Prepared {
                method: params.incoming().hints(),
                params: Payload::retain_or_copy(
                    params.incoming().payload()?.get_content(),
                    call.retain_params(messages, params.incoming().payload()?.get_content())
                        .map_err(wire_error)?,
                    caps,
                )?
                .into_params()
                .into(),
            })
        })
        .map_err(wire_error)?
    }
}
