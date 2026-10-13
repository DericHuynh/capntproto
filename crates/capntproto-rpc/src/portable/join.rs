//! Public equality routing using the same authenticated network profile as the
//! hosted driver. This handle retains neither tasks nor protocol storage.
use super::*;
use capnp::private::capability::{ClientHook, JoinedCapability};

pub struct Joiner<V: 'static> {
    connections: Weak<Connections<State>>,
    context: Weak<ContextOwner<V>>,
}
impl<V> Clone for Joiner<V> {
    fn clone(&self) -> Self {
        Self {
            connections: self.connections.clone(),
            context: self.context.clone(),
        }
    }
}
impl<V> Joiner<V> {
    pub(super) fn new(
        connections: Weak<Connections<State>>,
        context: Weak<ContextOwner<V>>,
    ) -> Self {
        Self {
            connections,
            context,
        }
    }
    pub fn join<T: capability::FromClientHook + 'static>(
        &self,
        caps: Vec<T>,
    ) -> Promise<Option<T>, Error> {
        let this = self.clone();
        Promise::from_future(async move {
            let result = this
                .resolve(caps.into_iter().map(T::into_client_hook).collect(), 0)
                .await?;
            if let Some(cap) = &result.cap {
                crate::rpc::join::check(&**cap)?;
            }
            Ok(result.cap.as_ref().map(|cap| T::new(cap.add_ref())))
        })
    }
    fn active(&self) -> capnp::Result<(Rc<Connections<State>>, Rc<ContextOwner<V>>)> {
        let connections = self.connections.upgrade().ok_or_else(closing)?;
        if connections.is_closing() {
            return Err(closing());
        }
        Ok((connections, self.context.upgrade().ok_or_else(closing)?))
    }
    fn resolve(
        &self,
        caps: Vec<Box<dyn ClientHook>>,
        depth: u8,
    ) -> Promise<JoinedCapability, Error> {
        let this = self.clone();
        Promise::from_future(async move {
            if depth > 64 || caps.is_empty() || caps.len() > u16::MAX as usize {
                return Err(Error::failed(
                    "Join requires 1..=65535 capabilities and bounded delegation".into(),
                ));
            }
            this.active()?;
            let caps = crate::rpc::join::settle_all(caps).await?;
            this.active()?;
            let identity = |cap: &dyn ClientHook| (cap.get_brand(), cap.get_ptr());
            if caps
                .iter()
                .all(|cap| identity(&**cap) == identity(&*caps[0]))
            {
                return Ok(JoinedCapability {
                    cap: Some(caps[0].add_ref()),
                    guard: Box::new(()),
                });
            }
            for cap in &caps {
                if let Some(delegation) = cap.delegate_join(&caps) {
                    let delegation = delegation?;
                    return (delegation.complete)(this.resolve(delegation.caps, depth + 1)).await;
                }
            }
            if caps.iter().all(|cap| cap.get_brand() == 0) {
                return Ok(JoinedCapability {
                    cap: None,
                    guard: Box::new(()),
                });
            }
            let network = this.active()?.1.join_network.clone().ok_or_else(|| {
                Error::unimplemented("authenticated Join network unavailable".into())
            })?;
            let session = network.start(caps.len() as u16)?;
            let mut replies = Vec::new();
            for (index, cap) in caps.into_iter().enumerate() {
                let part = session.part(index as u16)?;
                replies.push(crate::rpc::multiparty_join::part_at(
                    network.clone(),
                    cap,
                    part.get_root_as_reader()?,
                ));
            }
            let replies = futures::future::try_join_all(replies).await?;
            this.active()?;
            let Some(destination) = session.finish(&replies)? else {
                return Ok(JoinedCapability {
                    cap: None,
                    guard: Box::new(replies),
                });
            };
            let (cap, acquired) = match destination {
                crate::multiparty::JoinDestination::Local(exchange) => {
                    (exchange.accept(None).await?, None)
                }
                crate::multiparty::JoinDestination::Remote {
                    connection,
                    completion,
                } => {
                    let (connections, context) = this.active()?;
                    let state = super::connection::connect(&connections, &context, connection)?;
                    (state.idle)(false);
                    let response = state
                        .network_controls
                        .accept(completion.get_root_as_reader()?)
                        .await?;
                    let cap = response.get()?.get_as_capability()?;
                    (cap, Some(response))
                }
            };
            Ok(JoinedCapability {
                cap: Some(cap),
                guard: Box::new((replies, acquired)),
            })
        })
    }
}
