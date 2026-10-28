//! Standard rpc-twoparty Join over the same portable question/answer ledgers.
use super::*;
use crate::rpc_twoparty_capnp::{join_key_part, join_result};
use capnp::private::capability::JoinedCapability;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
pub(super) struct Ids {
    next: u64,
}
impl Ids {
    fn reserve(&mut self) -> capnp::Result<u32> {
        let id = u32::try_from(self.next)
            .map_err(|_| Error::overloaded("Join ID space exhausted".into()))?;
        self.next += 1;
        Ok(id)
    }
}
impl Handle {
    pub(crate) fn join_capabilities(
        &self,
        caps: Vec<Box<dyn ClientHook>>,
    ) -> Promise<JoinedCapability, Error> {
        let handle = self.clone();
        Promise::from_future(async move {
            let commands = handle.0.upgrade().ok_or_else(closed)?;
            if let Some(error) = commands.closed.borrow().clone() {
                return Err(error);
            }
            if !commands.bilateral.get() {
                return Err(Error::unimplemented(
                    "bilateral Join unavailable on this route".into(),
                ));
            }
            if caps.is_empty()
                || caps.len() > u16::MAX as usize
                || caps.len() > commands.capacity.limit()
            {
                return Err(Error::overloaded(
                    "bilateral Join exceeds connection capacity".into(),
                ));
            }
            let id = commands.join_ids.borrow_mut().reserve()?;
            let mut parts = Vec::new();
            parts.try_reserve_exact(caps.len()).map_err(wire_error)?;
            for index in 0..caps.len() {
                let mut part = Body::new_default();
                let mut key = part.init_root::<join_key_part::Builder>();
                key.set_join_id(id);
                key.set_part_count(caps.len() as u16);
                key.set_part_num(index as u16);
                parts.push(part);
            }
            let replies = caps
                .into_iter()
                .zip(parts.iter())
                .map(|(cap, part)| Ok(handle.join(cap, part.get_root_as_reader()?)))
                .collect::<capnp::Result<Vec<_>>>()?;
            let responses = futures::future::try_join_all(replies).await?;
            let mut succeeded = None;
            let mut cap = None;
            for response in &responses {
                let result = response.get()?.get_as::<join_result::Reader>()?;
                if result.get_join_id() != id
                    || succeeded.is_some_and(|old| old != result.get_succeeded())
                {
                    return Err(Error::failed("inconsistent Join results".into()));
                }
                succeeded = Some(result.get_succeeded());
                if result.has_cap() {
                    if !result.get_succeeded() || cap.is_some() {
                        return Err(Error::failed(
                            "Join returned unexpected capabilities".into(),
                        ));
                    }
                    cap = Some(result.get_cap().get_as_capability()?);
                }
            }
            if succeeded == Some(true) && cap.is_none() {
                return Err(Error::failed("successful Join omitted capability".into()));
            }
            Ok(JoinedCapability {
                cap,
                guard: Box::new(responses),
            })
        })
    }
}

type Outcome = capnp::Result<Rc<JoinedCapability>>;
type PartSender = shared::Sender<Outcome>;
type Running = futures::future::Shared<futures::future::LocalBoxFuture<'static, ()>>;
struct State {
    seen: BTreeSet<u16>,
    targets: BTreeMap<u16, Box<dyn ClientHook>>,
    senders: Vec<PartSender>,
    outcome: Option<Outcome>,
    running: Option<Running>,
}
struct Group {
    count: u16,
    state: RefCell<State>,
}
impl Group {
    fn publish(&self, result: Outcome) {
        let (senders, running, targets) = {
            let mut state = self.state.borrow_mut();
            if state.outcome.is_some() {
                return;
            }
            state.outcome = Some(result.clone());
            (
                std::mem::take(&mut state.senders),
                state.running.take(),
                std::mem::take(&mut state.targets),
            )
        };
        drop((running, targets));
        for sender in senders {
            let _ = sender.send(result.clone());
        }
    }
}
struct PartGuard(Rc<Group>);
impl Drop for PartGuard {
    fn drop(&mut self) {
        self.0
            .publish(Err(Error::failed("Join canceled before completion".into())));
    }
}
#[derive(Default)]
pub(super) struct Groups(BTreeMap<u32, Weak<Group>>);
struct JoinedReply {
    payload: Payload,
    _outcome: Rc<JoinedCapability>,
}
impl ResponseHook for JoinedReply {
    fn get(&self) -> capnp::Result<any_pointer::Reader<'_>> {
        self.payload.reader()
    }
}
impl Groups {
    pub(super) fn receive(
        &mut self,
        key: any_pointer::Reader<'_>,
        target: Box<dyn ClientHook>,
        maximum: usize,
        resolve: impl FnOnce(Vec<Box<dyn ClientHook>>) -> Promise<JoinedCapability, Error> + 'static,
    ) -> capnp::Result<(Box<dyn std::any::Any>, Promise<Completion, Error>)> {
        let key = key.get_as::<join_key_part::Reader>()?;
        let (id, count, index) = (key.get_join_id(), key.get_part_count(), key.get_part_num());
        if count == 0 || index >= count {
            return Err(Error::failed("invalid Join part number/count".into()));
        }
        if count as usize > maximum {
            return Err(Error::overloaded(
                "Join batch exceeds connection capacity".into(),
            ));
        }
        self.0.retain(|_, group| group.strong_count() != 0);
        let group = if let Some(group) = self.0.get(&id).and_then(Weak::upgrade) {
            group
        } else {
            if self.0.len() >= maximum {
                return Err(Error::overloaded("incoming Join groups exhausted".into()));
            }
            let group = Rc::new(Group {
                count,
                state: RefCell::new(State {
                    seen: BTreeSet::new(),
                    targets: BTreeMap::new(),
                    senders: Vec::new(),
                    outcome: None,
                    running: None,
                }),
            });
            self.0.insert(id, Rc::downgrade(&group));
            group
        };
        let (sender, mut receiver) = shared::channel();
        let ready = {
            let mut state = group.state.borrow_mut();
            if group.count != count || !state.seen.insert(index) {
                return Err(Error::failed("duplicate or inconsistent Join part".into()));
            }
            if let Some(outcome) = &state.outcome {
                let _ = sender.send(outcome.clone());
                false
            } else {
                state.senders.try_reserve(1).map_err(wire_error)?;
                state.targets.insert(index, target);
                state.senders.push(sender);
                state.seen.len() == count as usize
            }
        };
        if ready {
            let targets = std::mem::take(&mut group.state.borrow_mut().targets)
                .into_values()
                .collect();
            let future = resolve(targets);
            let weak = Rc::downgrade(&group);
            let running = async move {
                let result = future.await.map(Rc::new);
                if let Some(group) = weak.upgrade() {
                    group.publish(result)
                }
            }
            .boxed_local()
            .shared();
            group.state.borrow_mut().running = Some(running);
        }
        let guard = Box::new(PartGuard(group.clone()));
        let future = Promise::from_future(async move {
            let outcome = std::future::poll_fn(|cx| {
                // User hooks and wakers run after releasing the group borrow.
                let running = group.state.borrow_mut().running.take();
                if let Some(mut running) = running {
                    if Pin::new(&mut running).poll(cx).is_pending()
                        && group.state.borrow().outcome.is_none()
                    {
                        group.state.borrow_mut().running = Some(running);
                    }
                }
                Pin::new(&mut receiver).poll(cx)
            })
            .await
            .map_err(|_| closed())??;
            use capnp::traits::ImbueMut;
            let mut message = Body::new_default();
            let mut caps = Vec::new();
            let mut content = message.init_root::<any_pointer::Builder>();
            content.imbue_mut(&mut caps);
            let mut result = content.init_as::<join_result::Builder>();
            result.set_join_id(id);
            result.set_succeeded(outcome.cap.is_some());
            if index == 0 {
                if let Some(cap) = &outcome.cap {
                    result.get_cap().set_as_capability(cap.add_ref());
                }
            }
            Ok(Completion::Bilateral(Box::new(JoinedReply {
                payload: Payload {
                    body: PayloadBody::Owned(Box::new(message)),
                    caps,
                },
                _outcome: outcome,
            })))
        });
        Ok((guard, future))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn join_ids_do_not_wrap_before_queued_finishes_reach_the_wire() {
        let mut ids = Ids {
            next: u32::MAX as u64 - 1,
        };
        assert_eq!(ids.reserve().unwrap(), u32::MAX - 1);
        assert_eq!(ids.reserve().unwrap(), u32::MAX);
        for _ in 0..3 {
            assert_eq!(
                ids.reserve().unwrap_err().kind,
                capnp::ErrorKind::Overloaded
            );
        }
        assert_eq!(ids.next, 1u64 << 32);
    }
}
