//! Owner-mediated three-party introduction with a Native PSK and ordering barrier.
//! Packages must travel over existing authenticated capability connections.
#![forbid(unsafe_code)]
use crate::{
    authority::{Grant, Rights},
    semantics::{HandoffPhase, HandoffState},
    transport::{self, AuthenticatedSession, Identity},
};
use ring::{
    digest::{digest, SHA256},
    rand::SecureRandom,
};

/// An introduction with immutable recipient, host and key bindings.
/// Authentication is performed by [`serve`], never asserted by an application.
///
/// ```compile_fail,E0624
/// fn forge(intro: &mut capntproto::handoff::Introduction) {
///     intro.accept([1; 32], [2; 32]);
/// }
/// ```
/// ```compile_fail,E0616
/// fn replace_recipient(intro: &mut capntproto::handoff::Introduction) {
///     intro.recipient = [9; 32];
/// }
pub struct Introduction {
    id: [u8; 32],
    target: [u8; 32],
    recipient: [u8; 32],
    psk: [u8; 32],
    context: [u8; 32],
    grant: Grant,
    state: HandoffState,
}
/// Recipient's confidential connection material. Never serialize this to logs.
pub struct Package {
    pub id: [u8; 32],
    pub target: [u8; 32],
    pub recipient: [u8; 32],
    pub psk: [u8; 32],
    pub context: [u8; 32],
}
impl Introduction {
    pub fn new(
        owner: &Identity,
        parent: &Grant,
        recipient: [u8; 32],
        rights: Rights,
    ) -> Option<Self> {
        let grant = parent.delegate(recipient, rights)?;
        let mut id = [0; 32];
        let mut psk = [0; 32];
        let rng = ring::rand::SystemRandom::new();
        rng.fill(&mut id).ok()?;
        rng.fill(&mut psk).ok()?;
        let mut binding = b"ReProto introduction v1".to_vec();
        binding.extend_from_slice(&id);
        binding.extend_from_slice(&owner.public_key());
        binding.extend_from_slice(&recipient);
        binding.extend_from_slice(&grant.object().get().to_le_bytes());
        binding.extend_from_slice(&grant.generation().get().to_le_bytes());
        binding.push(rights.bits());
        let context = digest(&SHA256, &binding).as_ref().try_into().ok()?;
        Some(Self {
            id,
            target: owner.public_key(),
            recipient,
            psk,
            context,
            grant,
            state: HandoffState::default(),
        })
    }
    pub fn enqueue_proxy(&mut self) -> bool {
        self.state.enqueue()
    }
    pub fn finish_proxy(&mut self) -> bool {
        self.state.drain()
    }
    pub fn provide(&mut self) -> Option<Package> {
        if !self.state.provide() {
            return None;
        }
        Some(Package {
            id: self.id,
            target: self.target,
            recipient: self.recipient,
            psk: self.psk,
            context: self.context,
        })
    }
    fn accept(&mut self) -> bool {
        self.state.accept(self.grant.is_live())
    }
    pub fn lift_embargo(&mut self) -> bool {
        self.state.lift()
    }
    fn direct_grant(&self) -> Option<Grant> {
        if self.state.direct_ready() && self.grant.is_live() {
            Some(self.grant.clone())
        } else {
            None
        }
    }
    pub fn revoke(&mut self) {
        self.grant.revoke();
        let _ = self.state.revoke();
    }
    pub fn state(&self) -> HandoffState {
        self.state
    }
}

// Constructed only by serve(), after authentication, and installed only on that
// session's IO. The captured ID also prevents swapping the RefCell's complete
// Introduction value underneath an established or pending session.
struct Acceptor<T> {
    introduction: std::rc::Rc<std::cell::RefCell<Introduction>>,
    object: std::rc::Rc<crate::orm::ObjectState>,
    id: [u8; 32],
    cached: std::cell::RefCell<Option<crate::store_capnp::object::Client<T>>>,
}
impl<T: capnp::traits::Owned + Unpin + 'static> crate::store_capnp::handoff::Server<T>
    for Acceptor<T>
where
    for<'a> T::Reader<'a>: capnp::traits::SetterInput<T> + capnp::traits::HasTypeId,
{
    async fn accept(
        self: std::rc::Rc<Self>,
        params: crate::store_capnp::handoff::AcceptParams<T>,
        mut results: crate::store_capnp::handoff::AcceptResults<T>,
    ) -> capnp::Result<()> {
        let id = params.get()?.get_id()?;
        let grant = {
            let mut intro = self.introduction.borrow_mut();
            if id != self.id || intro.id != self.id || !intro.accept() {
                return Err(capnp::Error::failed(
                    "invalid or revoked introduction".into(),
                ));
            }
            if intro.state.phase() == HandoffPhase::Embargoed {
                intro.lift_embargo();
            }
            intro
                .direct_grant()
                .ok_or_else(|| capnp::Error::failed("old route has not drained".into()))?
        };
        let mut cached = self.cached.borrow_mut();
        if cached.is_none() {
            *cached = Some(crate::orm::ObjectServer::<T>::client(
                self.object.clone(),
                grant,
            )?);
        }
        results.get().set_object(cached.as_ref().unwrap().clone());
        Ok(())
    }
}

/// Owns the authenticated stream and its RPC task. Drop cancels both; no
/// detachable bootstrap or caller-supplied authentication labels are exposed.
#[must_use = "dropping the owner closes the handoff connection"]
pub struct Serving {
    pub(crate) session: AuthenticatedSession,
    pub(crate) rpc: tokio::task::JoinHandle<capnp::Result<()>>,
}
impl Serving {
    /// Wait until either driver ends. Cancellation closes both drivers.
    pub async fn wait(mut self) -> capnp::Result<()> {
        tokio::select! {
            result = &mut self.rpc => result.map_err(failure)?,
            result = &mut self.session.driver => result.map_err(failure)?.map_err(failure),
        }
    }
}
impl Drop for Serving {
    fn drop(&mut self) {
        self.rpc.abort();
    }
}
fn failure(error: impl std::fmt::Display) -> capnp::Error {
    capnp::Error::failed(error.to_string())
}

/// Authenticate a dedicated incoming connection using this introduction's host,
/// recipient, PSK and context, then install its bootstrap on the same stream.
/// The introduction must have been provided; its old-route fence may still be
/// pending. Run inside a Tokio LocalSet. Dropping the future cancels acceptance.
///
/// ```compile_fail,E0603
/// use capntproto::handoff::Acceptor;
/// ```
pub async fn serve<T: capnp::traits::Owned + Unpin + 'static>(
    introduction: std::rc::Rc<std::cell::RefCell<Introduction>>,
    object: std::rc::Rc<crate::orm::ObjectState>,
    owner: &Identity,
    socket: tokio::net::UdpSocket,
) -> capnp::Result<Serving>
where
    for<'a> T::Reader<'a>: capnp::traits::SetterInput<T> + capnp::traits::HasTypeId,
{
    let binding = {
        let intro = introduction.borrow();
        if owner.public_key() != intro.target
            || object.object() != intro.grant.object()
            || intro.state.phase() == HandoffPhase::Proxying
            || intro.state.is_revoked()
            || !intro.grant.is_live()
        {
            return Err(failure("invalid introduction binding"));
        }
        Package {
            id: intro.id,
            target: intro.target,
            recipient: intro.recipient,
            psk: intro.psk,
            context: intro.context,
        }
    };
    let mut session = transport::accept_authenticated(
        socket,
        owner,
        binding.recipient,
        Some(binding.psk),
        &binding.context,
    )
    .await
    .map_err(failure)?;
    {
        let intro = introduction.borrow();
        if intro.id != binding.id || intro.state.is_revoked() || !intro.grant.is_live() {
            return Err(failure(
                "introduction replaced or revoked during authentication",
            ));
        }
    }
    let bootstrap: crate::store_capnp::handoff::Client<T> = capnp_rpc::new_client(Acceptor {
        introduction,
        object,
        id: binding.id,
        cached: std::cell::RefCell::new(None),
    });
    let io = session.io.take().expect("new authenticated session");
    let rpc = crate::rpc::serve(io, bootstrap.client);
    Ok(Serving { session, rpc })
}

impl Drop for Introduction {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.psk.zeroize();
    }
}
impl Drop for Package {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.psk.zeroize();
    }
}
