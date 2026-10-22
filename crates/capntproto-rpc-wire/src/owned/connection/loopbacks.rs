use super::*;
impl<'pool, 'id, 's, I, A, P: Copy + Eq, S, const CAPS: usize, const PATH: usize>
    Driver<'pool, 'id, 's, I, A, P, S, CAPS, PATH>
{
    pub(super) fn normal_dispatch_available(&mut self) -> bool {
        let total = self.runtime.queued();
        let ordinary = self.runtime.ordinary_queued();
        // One ordinary admission plus one reserved control slot. When a control
        // already occupies that slot it must not force an unnecessary extra slot.
        let Some(required) = total
            .checked_add(1)
            .zip(ordinary.checked_add(2))
            .map(|(all, calls)| all.max(calls))
        else {
            return false;
        };
        self.runtime.reserve_task_capacity(required)
    }
    pub(super) fn reserve_barrier(&mut self) -> Result<(), Error> {
        if self.barrier.is_some() || !self.loopbacks.reserve_capacity() {
            Err(Error::BarrierFull)
        } else {
            Ok(())
        }
    }
    pub(super) fn hold_barrier(&mut self, barrier: clients::Barrier<'id>) {
        // reserve_barrier() precedes the graph transition which can create this
        // affine obligation. No callback can consume its reserved storage.
        self.barrier = Some(
            self.loopbacks
                .register(barrier)
                .unwrap_or_else(|_| unreachable!("preflighted loopback capacity")),
        );
    }
    pub(super) fn retry_barrier(&mut self) -> Result<bool, Error> {
        let Some(ticket) = self.barrier.as_ref() else {
            return Ok(false);
        };
        let route = self
            .runtime
            .retain_barrier_route(self.loopbacks.barrier(ticket).map_err(Error::Loopback)?)
            .map_err(Error::Graph)?;
        let mut task = self.barrier.take().map(Task::Loopback);
        let result = self
            .runtime
            .prepare_call(&route, &mut task)
            .map(|pending| pending.reserved_control().accept());
        self.runtime.release(route);
        match result {
            Ok(_) => Ok(true),
            Err(error) => {
                let Some(Task::Loopback(ticket)) = task else {
                    unreachable!("unaccepted loopback")
                };
                self.barrier = Some(ticket);
                Err(Error::Dispatch(error))
            }
        }
    }
    pub(super) fn retry_echo(&mut self) -> Result<bool, Error> {
        if self.echo.is_none() {
            return Ok(false);
        }
        if !self.normal_dispatch_available() {
            return Err(Error::Dispatch(dispatch::DispatchError::Full));
        }
        let echo = self.echo.as_ref().unwrap();
        let (route, observer) = match echo.target() {
            TargetRef::Hosted(lease) => (
                binding::reflected(&mut self.runtime, &self.ledgers.exports, lease)
                    .map_err(Error::Graph)?,
                false,
            ),
            TargetRef::Promised { answer, path } => (
                self.pipelines
                    .bind(&mut self.runtime, &mut self.ledgers.answers, answer, path)
                    .map_err(Error::Pipeline)?,
                true,
            ),
        };
        let key = route.key();
        let mut task = self.echo.take().map(Task::Echo);
        let result = self
            .runtime
            .prepare_call(&route, &mut task)
            .map(|pending| pending.accept());
        if result.is_err() && observer {
            self.pipelines
                .discard_unexposed(key, &mut self.ledgers.answers, &mut self.runtime);
        }
        self.runtime.release(route);
        match result {
            Ok(_) => Ok(true),
            Err(error) => {
                let Some(Task::Echo(echo)) = task else {
                    unreachable!("unaccepted echo")
                };
                self.echo = Some(echo);
                Err(Error::Dispatch(error))
            }
        }
    }
    pub(super) fn send_loopback(
        &mut self,
        messages: &mut messages::Messages<'pool, '_, '_>,
        queue: &mut messages::Queue<'pool, '_, impl Default>,
        minimum: usize,
        options: capnp::message::ReaderOptions,
    ) -> Result<bool, Error> {
        let Some(ready) = self.runtime.ready().map_err(Error::Dispatch)? else {
            return Ok(false);
        };
        if matches!(
            ready.owner(),
            Task::Call(_) | Task::Invoke(_) | Task::ProvisionBarrier(_) | Task::VineBarrier(_)
        ) {
            return Ok(false);
        }
        let clients::Destination::Endpoint {
            owner,
            location: clients::Location::Remote(peer),
            ..
        } = ready.destination()
        else {
            return Err(Error::InvalidLoopbackTarget);
        };
        if peer != self.imports.peer() {
            return Err(Error::InvalidLoopbackTarget);
        }
        let target = owner
            .target(&self.ledgers.questions)
            .map_err(Error::Question)?
            .ok_or(Error::InvalidLoopbackTarget)?;
        let output = queue
            .reserve()
            .map_err(|error| Error::Input(crate::owned::Error::Storage(error)))?;
        let (frame, receipt) = match ready.owner() {
            Task::Loopback(ticket) => {
                let receipt = self.loopbacks.prepare(ticket).map_err(Error::Loopback)?;
                let frame = Frame::build(messages, minimum, options, |builder| {
                    calls::sender_loopback(builder, receipt.id(), target)
                })
                .map_err(Error::Input)?;
                (frame, Some(receipt))
            }
            Task::Echo(echo) => (
                Frame::build(messages, minimum, options, |builder| {
                    calls::receiver_loopback(builder, echo.context(), target)
                })
                .map_err(Error::Input)?,
                None,
            ),
            Task::Call(_) | Task::Invoke(_) | Task::ProvisionBarrier(_) | Task::VineBarrier(_) => {
                unreachable!()
            }
        };
        let offer = match messages.offer(&frame.lease) {
            Ok(offer) => offer,
            Err(error) => {
                frame.release(messages);
                return Err(Error::Input(crate::owned::Error::Storage(error)));
            }
        };
        // Everything following this point is infallible. Output admission and
        // removal from ordered dispatch commit together, before target release.
        let running = ready.accept();
        if let Some(receipt) = receipt {
            receipt.accept();
        }
        output.accept(messages, offer);
        frame.release(messages);
        if let Task::Echo(echo) = self.runtime.complete(running) {
            echo.release(&mut self.ledgers.exports, &mut self.ledgers.answers);
        }
        Ok(true)
    }
}
