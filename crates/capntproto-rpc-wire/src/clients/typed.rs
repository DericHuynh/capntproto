//! Schema identity on retained callable graph routes. Explicit interface casts
//! remain possible; invoking an unsupported interface fails through normal RPC.
use capntproto_rpc_engine::clients;
use core::marker::PhantomData;

/// A generated field's interface stays attached to the owning client.
///
/// ```compile_fail
/// use capntproto_rpc_wire::clients::{CallTarget, TypedClient};
/// fn wrong_interface<'id, I, J>(client: &TypedClient<'id, I>) {
///     let _: &dyn CallTarget<'id, J> = client;
/// }
/// ```
#[must_use = "release the client through its originating session or graph"]
pub struct TypedClient<'id, I> {
    client: clients::Client<'id>,
    marker: PhantomData<fn() -> I>,
}
impl<'id, I> TypedClient<'id, I> {
    /// Explicitly assign an interface to a low-level graph capability. This
    /// does not claim that the remote service implements that interface.
    pub fn from_client(client: clients::Client<'id>) -> Self {
        Self {
            client,
            marker: PhantomData,
        }
    }
    pub fn as_client(&self) -> &clients::Client<'id> {
        &self.client
    }
    pub fn into_client(self) -> clients::Client<'id> {
        self.client
    }
    pub fn cast<T>(self) -> TypedClient<'id, T> {
        TypedClient::from_client(self.client)
    }
}
impl<I> core::fmt::Debug for TypedClient<'_, I> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_tuple("TypedClient")
            .field(&self.client.key())
            .finish()
    }
}
mod sealed {
    pub trait Target {}
}
impl<I> sealed::Target for TypedClient<'_, I> {}
impl sealed::Target for clients::Client<'_> {}
/// Typed clients accept only methods from their interface. Raw graph clients
/// remain an explicit low-level escape hatch for dynamic protocol adapters.
pub trait CallTarget<'id, I>: sealed::Target {
    fn as_call_target(&self) -> &clients::Client<'id>;
}
impl<'id, I> CallTarget<'id, I> for TypedClient<'id, I> {
    fn as_call_target(&self) -> &clients::Client<'id> {
        &self.client
    }
}
impl<'id, I> CallTarget<'id, I> for clients::Client<'id> {
    fn as_call_target(&self) -> &clients::Client<'id> {
        self
    }
}
