//! Generated services invoked through concrete legacy CallContext ownership.
//! This fixture does not instantiate the bounded or hosted protocol driver.
#![no_std]
#![forbid(unsafe_code)]
capnp::generated_code!(pub mod legacy_capnp);
use capnp::{
    any_pointer,
    capability::{dispatch_call, dispatch_call_with_context, CallContext, PreparedCall, Promise},
    field_api::{FixedField, FixedStruct, Generic},
    message,
    private::capability::ParamsHook,
    wire_capability::Service,
    Error, ErrorKind, Result,
};
use capntproto_rpc_engine::{
    completion, tasks,
    wake::{Static, Wake},
};
use capntproto_rpc_wire::{
    calls,
    rpc_capnp::{self, api},
};
use core::{
    cell::Cell,
    future::Future,
    pin::Pin,
    sync::atomic::{AtomicUsize, Ordering},
    task::{Context, Poll, Waker},
};
use legacy_capnp::{base, node};
const ZERO: capnp::Word = capnp::word(0, 0, 0, 0, 0, 0, 0, 0);
struct Notice(&'static AtomicUsize);
impl Wake for Notice {
    fn wake_by_ref(&self) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}
struct Input<'a, S: message::ReaderSegments> {
    reader: message::Reader<S>,
    drops: &'a Cell<usize>,
}
impl<S: message::ReaderSegments> ParamsHook for Input<'_, S> {
    fn get(&self) -> Result<any_pointer::Reader<'_>> {
        let root = self.reader.get_root::<rpc_capnp::message::Reader<'_>>()?;
        let rpc_capnp::message::Call(call) = root.which()? else {
            return Err(Error::from_kind(ErrorKind::Failed));
        };
        Ok(call?.get_params()?.get_content())
    }
}
impl<S: message::ReaderSegments> Drop for Input<'_, S> {
    fn drop(&mut self) {
        self.drops.set(self.drops.get() + 1);
    }
}
struct MethodGuard<'a> {
    params: &'a Cell<usize>,
    retired: &'a Cell<usize>,
}
impl Drop for MethodGuard<'_> {
    fn drop(&mut self) {
        assert_eq!(
            self.params.get(),
            0,
            "input owner destroyed before suspended method"
        );
        self.retired.set(self.retired.get() + 1);
    }
}
struct Server<'a> {
    gate: Option<completion::Receiver<'a, ()>>,
    params: &'a Cell<usize>,
    retired: &'a Cell<usize>,
    started: &'a Cell<usize>,
}
impl base::Server for Server<'_> {
    async fn read<'call>(
        &'call mut self,
        params: legacy_capnp::api::base::ReadParamsRef<'call>,
        mut results: FixedStruct<'call, legacy_capnp::api::base::ReadResults>,
    ) -> Result<()> {
        self.started.set(self.started.get() + 1);
        let _guard = MethodGuard {
            params: self.params,
            retired: self.retired,
        };
        if let Some(wait) = self.gate.take() {
            wait.await
                .map_err(|_| Error::from_kind(ErrorKind::Disconnected))?;
        }
        results
            .scalar(legacy_capnp::api::base::ReadResults::VALUE)
            .set(params.value() + 1);
        Ok(())
    }
}
impl node::Server for Server<'_> {
    async fn hold<'call>(
        &'call mut self,
        params: legacy_capnp::api::node::HoldParamsRef<'call>,
        mut results: FixedStruct<'call, legacy_capnp::api::node::HoldResults>,
    ) -> Result<()> {
        self.started.set(self.started.get() + 1);
        let _guard = MethodGuard {
            params: self.params,
            retired: self.retired,
        };
        if let Some(wait) = self.gate.take() {
            wait.await
                .map_err(|_| Error::from_kind(ErrorKind::Disconnected))?;
        }
        results
            .scalar(legacy_capnp::api::node::HoldResults::VALUE)
            .set(params.value());
        Ok(())
    }
}
type Invocation<'a, P> = CallContext<
    'a,
    any_pointer::Owned,
    any_pointer::Owned,
    PreparedCall<P, FixedField<'a, Generic<any_pointer::Owned>>>,
>;
fn invocation<'a, P>(
    params: P,
    output: &'a mut message::FixedBuilder<'_>,
) -> Result<Invocation<'a, P>> {
    let root = output.try_init_root::<api::Message>()?;
    let mut returned = root.into_field(api::Message::RETURN).init()?;
    returned.scalar(api::Return::ANSWER_ID).set(37);
    let content = returned
        .into_field(api::Return::RESULTS)
        .init()?
        .into_field(api::Payload::CONTENT);
    Ok(CallContext::from_owner(PreparedCall::new(params, content)))
}
fn source<'a>(words: &'a mut [capnp::Word], hold: bool) -> Result<message::FixedBuilder<'a>> {
    use capntproto_rpc_engine::ids::{ImportId, QuestionId, WireId};
    let mut message = message::FixedBuilder::new(words)?;
    if hold {
        calls::call(
            &mut message,
            QuestionId::from_wire(37),
            ImportId::from_wire(1),
            node::Owned::HOLD,
            |mut params| {
                params
                    .scalar(legacy_capnp::api::node::HoldParams::VALUE)
                    .set(41);
                Ok(())
            },
        )?;
    } else {
        calls::call(
            &mut message,
            QuestionId::from_wire(37),
            ImportId::from_wire(1),
            base::Owned::READ,
            |mut params| {
                params
                    .scalar(legacy_capnp::api::base::ReadParams::VALUE)
                    .set(41);
                Ok(())
            },
        )?;
    }
    Ok(message)
}
pub fn retained_generated_invocation() -> Result<()> {
    static WAKES: AtomicUsize = AtomicUsize::new(0);
    static NOTICE: Static<Notice> = Static::new(Notice(&WAKES));
    let mut words = [ZERO; 32];
    let input = source(&mut words, false)?;
    let mut output_words = [ZERO; 32];
    let mut output = message::FixedBuilder::new(&mut output_words)?;
    let params_dropped = Cell::new(0);
    let retired = Cell::new(0);
    let started = Cell::new(0);
    let mut gate = completion::Slot::new();
    let (ready, wait) = gate.pair().unwrap();
    let mut service = node::ServerDispatch::new(Server {
        gate: Some(wait),
        params: &params_dropped,
        retired: &retired,
        started: &started,
    });
    assert!(
        service
            .method_info(base::Owned::READ.interface_id(), 0)
            .unwrap()
            .allow_cancellation
    );
    let owned = Input {
        reader: message::Reader::new(&input, Default::default()),
        drops: &params_dropped,
    };
    let call = invocation(owned, &mut output)?;
    {
        let mut places = core::pin::pin!(tasks::PinnedSlots::<_, 1>::new());
        let [place] = places.as_mut().places();
        let mut task = Promise::from_owner(
            place
                .insert_erased(dispatch_call(
                    call,
                    &mut service,
                    base::Owned::READ.interface_id(),
                    0,
                ))
                .ok()
                .unwrap(),
        );
        let waker = NOTICE.waker();
        let mut cx = Context::from_waker(&waker);
        assert_eq!(started.get(), 0);
        assert!(Pin::new(&mut task).poll(&mut cx).is_pending());
        assert_eq!(started.get(), 1);
        assert_eq!(params_dropped.get(), 0);
        let wakes = WAKES.load(Ordering::Relaxed);
        assert!(ready.send(()).is_ok());
        assert!(WAKES.load(Ordering::Relaxed) > wakes);
        assert!(matches!(
            Pin::new(&mut task).poll(&mut cx),
            Poll::Ready(Ok(()))
        ));
        drop(task);
        assert!(places.is_empty());
        assert_eq!(params_dropped.get(), 1);
        assert_eq!(retired.get(), 1);
    }
    let root = output.get_root_as_reader::<rpc_capnp::message::Reader<'_>>()?;
    let rpc_capnp::message::Return(returned) = root.which()? else {
        panic!("Return lost")
    };
    let returned = returned?;
    assert_eq!(returned.get_answer_id(), 37);
    let rpc_capnp::return_::Results(payload) = returned.which()? else {
        panic!("results lost")
    };
    assert_eq!(
        payload?
            .get_content()
            .get_as::<legacy_capnp::base::read_results::Reader<'_>>()?
            .get_value(),
        42
    );
    Ok(())
}
pub fn cancel_generated_invocation() -> Result<()> {
    let mut words = [ZERO; 32];
    let input = source(&mut words, true)?;
    let mut output_words = [ZERO; 32];
    let mut output = message::FixedBuilder::new(&mut output_words)?;
    let params_dropped = Cell::new(0);
    let retired = Cell::new(0);
    let started = Cell::new(0);
    let mut gate = completion::Slot::new();
    let (ready, wait) = gate.pair().unwrap();
    let mut service = node::ServerDispatch::new(Server {
        gate: Some(wait),
        params: &params_dropped,
        retired: &retired,
        started: &started,
    });
    let owned = Input {
        reader: message::Reader::new(&input, Default::default()),
        drops: &params_dropped,
    };
    let call = invocation(owned, &mut output)?;
    let mut places = core::pin::pin!(tasks::PinnedSlots::<_, 1>::new());
    let [place] = places.as_mut().places();
    let mut task = Promise::from_owner(
        place
            .insert_erased(dispatch_call(
                call,
                &mut service,
                node::Owned::HOLD.interface_id(),
                0,
            ))
            .ok()
            .unwrap(),
    );
    assert!(Pin::new(&mut task)
        .poll(&mut Context::from_waker(Waker::noop()))
        .is_pending());
    assert_eq!(started.get(), 1);
    drop(task);
    assert!(places.is_empty());
    assert_eq!(retired.get(), 1);
    assert_eq!(params_dropped.get(), 1);
    assert!(ready.send(()).is_err());
    Ok(())
}
struct Contextual;
impl base::ContextServer<u64> for Contextual {
    async fn read<'call>(
        &'call mut self,
        params: legacy_capnp::api::base::ReadParamsRef<'call>,
        mut results: FixedStruct<'call, legacy_capnp::api::base::ReadResults>,
        context: &'call mut u64,
    ) -> Result<()> {
        let mut yielded = false;
        core::future::poll_fn(|cx| {
            if yielded {
                Poll::Ready(())
            } else {
                yielded = true;
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        })
        .await;
        *context += params.value();
        results
            .scalar(legacy_capnp::api::base::ReadResults::VALUE)
            .set(*context);
        Ok(())
    }
}
impl node::ContextServer<u64> for Contextual {}
pub fn contextual_generated_invocation() -> Result<()> {
    let mut words = [ZERO; 32];
    let input = source(&mut words, false)?;
    let mut output_words = [ZERO; 32];
    let mut output = message::FixedBuilder::new(&mut output_words)?;
    let dropped = Cell::new(0);
    let owned = Input {
        reader: message::Reader::new(&input, Default::default()),
        drops: &dropped,
    };
    let mut service = node::ContextServerDispatch::new(Contextual);
    let mut context = 1u64;
    let call = invocation(owned, &mut output)?;
    {
        let task = dispatch_call_with_context(
            call,
            &mut service,
            base::Owned::READ.interface_id(),
            0,
            &mut context,
        );
        let mut task = core::pin::pin!(task);
        let mut cx = Context::from_waker(Waker::noop());
        assert!(task.as_mut().poll(&mut cx).is_pending());
        assert!(matches!(task.as_mut().poll(&mut cx), Poll::Ready(Ok(()))));
    }
    assert_eq!(context, 42);
    assert_eq!(dropped.get(), 1);
    Ok(())
}
struct Invalid<'a>(&'a Cell<usize>);
impl ParamsHook for Invalid<'_> {
    fn get(&self) -> Result<any_pointer::Reader<'_>> {
        self.0.set(self.0.get() + 1);
        Err(Error::from_kind(ErrorKind::Failed))
    }
}
pub fn reject_before_application() -> Result<()> {
    let dropped = Cell::new(0);
    let retired = Cell::new(0);
    let started = Cell::new(0);
    let gets = Cell::new(0);
    let mut service = node::ServerDispatch::new(Server {
        gate: None,
        params: &dropped,
        retired: &retired,
        started: &started,
    });
    let mut words = [ZERO; 32];
    let mut output = message::FixedBuilder::new(&mut words)?;
    for (interface, method, kind, reads) in [
        (0, 0, ErrorKind::Unimplemented, 0),
        (
            base::Owned::READ.interface_id(),
            99,
            ErrorKind::Unimplemented,
            0,
        ),
        (base::Owned::READ.interface_id(), 0, ErrorKind::Failed, 1),
    ] {
        output.reset();
        let call = invocation(Invalid(&gets), &mut output)?;
        {
            let task = dispatch_call(call, &mut service, interface, method);
            let mut task = core::pin::pin!(task);
            let Poll::Ready(Err(e)) = task.as_mut().poll(&mut Context::from_waker(Waker::noop()))
            else {
                panic!("invalid dispatch accepted")
            };
            assert_eq!(e.kind, kind);
        }
        assert_eq!(gets.get(), reads);
        assert_eq!(started.get(), 0);
        let root = output.get_root_as_reader::<rpc_capnp::message::Reader<'_>>()?;
        let rpc_capnp::message::Return(returned) = root.which()? else {
            panic!("Return lost")
        };
        let rpc_capnp::return_::Results(payload) = returned?.which()? else {
            panic!("results lost")
        };
        assert!(payload?.get_content().is_null());
    }
    Ok(())
}
struct Shared {
    active: Cell<usize>,
    peak: Cell<usize>,
}
struct Active<'a>(&'a Shared);
impl Drop for Active<'_> {
    fn drop(&mut self) {
        self.0.active.set(self.0.active.get() - 1);
    }
}
impl base::Server for &Shared {
    async fn read<'call>(
        &'call mut self,
        params: legacy_capnp::api::base::ReadParamsRef<'call>,
        mut results: FixedStruct<'call, legacy_capnp::api::base::ReadResults>,
    ) -> Result<()> {
        self.active.set(self.active.get() + 1);
        self.peak.set(self.peak.get().max(self.active.get()));
        let _active = Active(self);
        let mut yielded = false;
        core::future::poll_fn(|cx| {
            if yielded {
                Poll::Ready(())
            } else {
                yielded = true;
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        })
        .await;
        results
            .scalar(legacy_capnp::api::base::ReadResults::VALUE)
            .set(params.value() + 1);
        Ok(())
    }
}
impl node::Server for &Shared {}
/// Each future owns its generated adapter. All adapters borrow the same server;
/// no future borrows a dispatcher or context that contains that future.
pub fn concurrent_generated_invocations() -> Result<()> {
    const N: usize = 32;
    static WAKES: AtomicUsize = AtomicUsize::new(0);
    static NOTICE: Static<Notice> = Static::new(Notice(&WAKES));
    let shared = Shared {
        active: Cell::new(0),
        peak: Cell::new(0),
    };
    let mut input_words = [[ZERO; 32]; N];
    let inputs = input_words
        .each_mut()
        .map(|words| source(words, false).unwrap());
    let mut output_words = [[ZERO; 32]; N];
    let mut outputs = output_words
        .each_mut()
        .map(|words| message::FixedBuilder::new(words).unwrap());
    let dropped = [const { Cell::new(0) }; N];
    {
        let mut places = core::pin::pin!(tasks::PinnedSlots::<_, N>::new());
        let mut entries = places
            .as_mut()
            .places()
            .into_iter()
            .zip(inputs.iter())
            .zip(outputs.iter_mut())
            .zip(dropped.iter());
        let mut operations = core::array::from_fn::<_, N, _>(|_| {
            let (((place, input), output), drops) = entries.next().unwrap();
            let owner = Input {
                reader: message::Reader::new(input, Default::default()),
                drops,
            };
            let call = invocation(owner, output).unwrap();
            Promise::from_owner(
                place
                    .insert_erased(dispatch_call(
                        call,
                        node::ServerDispatch::new(&shared),
                        base::Owned::READ.interface_id(),
                        0,
                    ))
                    .ok()
                    .unwrap(),
            )
        });
        drop(entries);
        let waker = NOTICE.waker();
        let mut cx = Context::from_waker(&waker);
        let wakes = WAKES.load(Ordering::Relaxed);
        for operation in &mut operations {
            assert!(Pin::new(operation).poll(&mut cx).is_pending());
        }
        assert_eq!(shared.active.get(), N);
        assert_eq!(shared.peak.get(), N);
        assert!(WAKES.load(Ordering::Relaxed) >= wakes + N);
        assert!(dropped.iter().all(|d| d.get() == 0));
        for operation in &mut operations {
            assert!(matches!(
                Pin::new(operation).poll(&mut cx),
                Poll::Ready(Ok(()))
            ));
        }
        assert_eq!(shared.active.get(), 0);
        drop(operations);
        assert!(places.is_empty());
    }
    assert!(dropped.iter().all(|d| d.get() == 1));
    for output in &outputs {
        let root = output.get_root_as_reader::<rpc_capnp::message::Reader<'_>>()?;
        let rpc_capnp::message::Return(returned) = root.which()? else {
            panic!("Return lost")
        };
        let rpc_capnp::return_::Results(payload) = returned?.which()? else {
            panic!("results lost")
        };
        assert_eq!(
            payload?
                .get_content()
                .get_as::<legacy_capnp::base::read_results::Reader<'_>>()?
                .get_value(),
            42
        );
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn thirty_two_generated_calls_share_one_service_without_allocating() {
        let m = allocation_counter::measure(|| concurrent_generated_invocations().unwrap());
        assert_eq!((m.count_total, m.bytes_total), (0, 0));
    }
    #[test]
    fn retained_generated_dispatch_has_no_allocations() {
        let m = allocation_counter::measure(|| retained_generated_invocation().unwrap());
        assert_eq!((m.count_total, m.bytes_total), (0, 0));
    }
    #[test]
    fn cancellation_destroys_generated_future_before_input_owner() {
        let m = allocation_counter::measure(|| cancel_generated_invocation().unwrap());
        assert_eq!((m.count_total, m.bytes_total), (0, 0));
    }
    #[test]
    fn generated_context_dispatch_borrows_independent_context() {
        let m = allocation_counter::measure(|| contextual_generated_invocation().unwrap());
        assert_eq!((m.count_total, m.bytes_total), (0, 0));
    }
    #[test]
    fn invalid_calls_reject_before_parameter_access_or_application_entry() {
        let m = allocation_counter::measure(|| reject_before_application().unwrap());
        assert_eq!((m.count_total, m.bytes_total), (0, 0));
    }
}
