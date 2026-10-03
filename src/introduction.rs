//! Explicit three-party connection establishment over authenticated RPC.
//! Provide follows the caller's old-route completion fence; it is not a global
//! ordering barrier for arbitrary concurrent connections.
#![forbid(unsafe_code)]
use crate::{
    authority::{Grant, Rights},
    handoff::{Introduction, Serving},
    orm::ObjectState,
    rpc,
    store_capnp::{handoff, introducer, introduction_ticket, object},
    transport::{self, Identity},
};
use capnp::traits::{HasTypeId, Owned, SetterInput};
use std::{
    cell::{Cell, RefCell},
    marker::PhantomData,
    net::SocketAddr,
    rc::Rc,
};

pub struct Introducer<T> {
    pub owner: Rc<Identity>,
    pub parent: Grant,
    pub object: Rc<ObjectState>,
    pub rights: Rights,
    pub bind: SocketAddr,
    active: Rc<Cell<usize>>,
    _type: PhantomData<T>,
}
impl<T: Owned + Unpin + 'static> Introducer<T>
where
    for<'a> T::Reader<'a>: SetterInput<T> + HasTypeId,
{
    pub fn client(
        owner: Rc<Identity>,
        parent: Grant,
        object: Rc<ObjectState>,
        rights: Rights,
        bind: SocketAddr,
    ) -> introducer::Client<T> {
        capnp_rpc::new_client(Self {
            owner,
            parent,
            object,
            rights,
            bind,
            active: Rc::new(Cell::new(0)),
            _type: PhantomData,
        })
    }
}
struct Active(Rc<Cell<usize>>);
impl Drop for Active {
    fn drop(&mut self) {
        self.0.set(self.0.get() - 1);
    }
}
impl<T: Owned + Unpin + 'static> introducer::Server<T> for Introducer<T>
where
    for<'a> T::Reader<'a>: SetterInput<T> + HasTypeId,
{
    async fn provide(
        self: Rc<Self>,
        params: introducer::ProvideParams<T>,
        mut results: introducer::ProvideResults<T>,
    ) -> capnp::Result<()> {
        let recipient: [u8; 32] = params
            .get()?
            .get_recipient()?
            .try_into()
            .map_err(|_| capnp::Error::failed("recipient must be Ed25519 key".into()))?;
        if self.active.get() >= 64 {
            return Err(capnp::Error::overloaded("introduction limit".into()));
        }
        let mut introduction = Introduction::new(&self.owner, &self.parent, recipient, self.rights)
            .ok_or_else(|| capnp::Error::failed("delegation not permitted".into()))?;
        let ticket = introduction.provide().unwrap();
        let socket = tokio::net::UdpSocket::bind(self.bind)
            .await
            .map_err(failed)?;
        let address = socket.local_addr().map_err(failed)?;
        if address.ip().is_unspecified() {
            return Err(capnp::Error::failed(
                "provide requires a concrete advertised bind address".into(),
            ));
        }
        let mut out = results.get().init_ticket();
        out.set_id(&ticket.id);
        out.set_target(&ticket.target);
        out.set_recipient(&recipient);
        out.set_psk(&ticket.psk);
        out.set_context(&ticket.context);
        out.set_address(address.to_string());
        self.active.set(self.active.get() + 1);
        let active = Active(self.active.clone());
        let object = self.object.clone();
        let owner = self.owner.clone();
        tokio::task::spawn_local(async move {
            let _active = active;
            let Ok(serving) = crate::handoff::serve::<T>(
                Rc::new(RefCell::new(introduction)),
                object,
                &owner,
                socket,
            )
            .await
            else {
                return;
            };
            let _ = serving.wait().await;
        });
        Ok(())
    }
}
fn failed(e: impl std::fmt::Display) -> capnp::Error {
    capnp::Error::failed(e.to_string())
}
/// Owns network/RPC tasks; dropping the session closes the local connection.
pub struct Direct<T> {
    pub object: object::Client<T>,
    _connection: Serving,
}
/// Redeem a ticket delivered over an authenticated capability channel.
pub async fn connect<T: Owned + Unpin + 'static>(
    identity: &Identity,
    ticket: introduction_ticket::Reader<'_>,
    local: SocketAddr,
) -> capnp::Result<Direct<T>> {
    let id = ticket.get_id()?.to_vec();
    if id.len() != 32 {
        return Err(failed("invalid introduction ID"));
    }
    let target = ticket
        .get_target()?
        .try_into()
        .map_err(|_| failed("invalid target key"))?;
    if ticket.get_recipient()? != identity.public_key() {
        return Err(failed("ticket belongs to another recipient"));
    }
    let psk = ticket
        .get_psk()?
        .try_into()
        .map_err(|_| failed("invalid introduction PSK"))?;
    let context = ticket.get_context()?.to_vec();
    if context.len() != 32 {
        return Err(failed("invalid context"));
    }
    let address = ticket
        .get_address()?
        .to_str()
        .map_err(failed)?
        .parse()
        .map_err(failed)?;
    let socket = tokio::net::UdpSocket::bind(local).await.map_err(failed)?;
    let mut session =
        transport::connect_authenticated(socket, address, identity, target, Some(psk), &context)
            .await
            .map_err(failed)?;
    let io = session.io.take().expect("new authenticated session");
    let (client, task) = rpc::client::<handoff::Client<T>>(io);
    let connection = Serving { session, rpc: task };
    let mut request = client.accept_request();
    request.get().set_id(&id);
    let response = tokio::time::timeout(std::time::Duration::from_secs(10), request.send().promise)
        .await
        .map_err(failed)??;
    let object = response.get()?.get_object()?;
    Ok(Direct {
        object,
        _connection: connection,
    })
}

/// A deliberately delegated capability to introduce this recipient to peers.
/// Completed direct sessions are delivered through a bounded application queue.
pub struct Receiver<T> {
    pub identity: Rc<Identity>,
    pub local: SocketAddr,
    pub accepted: tokio::sync::mpsc::Sender<Direct<T>>,
}
impl<T: Owned + Unpin + 'static> crate::store_capnp::introduction_receiver::Server<T>
    for Receiver<T>
{
    async fn introduce(
        self: Rc<Self>,
        params: crate::store_capnp::introduction_receiver::IntroduceParams<T>,
        _: crate::store_capnp::introduction_receiver::IntroduceResults<T>,
    ) -> capnp::Result<()> {
        let permit = self
            .accepted
            .try_reserve()
            .map_err(|_| failed("recipient introduction queue full"))?;
        let p = params.get()?;
        let session = connect::<T>(&self.identity, p.get_ticket()?, self.local).await?;
        permit.send(session);
        Ok(())
    }
}
