//! A call may resolve locally or on a peer without changing its completion slot.
use super::*;

#[must_use = "release the completed output through its originating driver"]
pub enum CallOutput<'pool, 'id, I, const CAPS: usize> {
    Remote(Response<'pool, 'id, I, CAPS>),
    Local(LocalResult<'pool, 'id, CAPS>),
}
/// Local dispatch failures use the same portable error kinds as broken routes.
/// A remote protocol exception retains its complete wire representation in the
/// remote response, including the peer's reason and optional trace.
pub type CallResult<'pool, 'id, I, const CAPS: usize> =
    Result<CallOutput<'pool, 'id, I, CAPS>, capnp::ErrorKind>;
impl<'pool, 'id, 's, I, A, P: Copy + Eq, S, const CAPS: usize, const PATH: usize>
    Driver<'pool, 'id, 's, I, A, P, S, CAPS, PATH>
{
    pub fn release_output(
        &mut self,
        output: CallOutput<'pool, 'id, I, CAPS>,
        messages: &mut messages::Messages<'pool, '_, '_>,
    ) {
        match output {
            CallOutput::Remote(response) => self.release_response(response, messages),
            CallOutput::Local(result) => result.release(messages, &mut self.runtime),
        }
    }
}
