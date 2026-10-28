//! Bootstrap uses the same answer and pipeline ownership as method calls, but
//! can only complete from its own admitted token and returns one capability.
use super::*;

/// An admitted Bootstrap in this connection's exact answer generation. Keep
/// the token until its Return is accepted, or terminate the connection. It
/// cannot be constructed from a wire ID or an ordinary method's answer key.
#[must_use = "complete and publish this bootstrap, or terminate the connection"]
pub struct Bootstrap<'id> {
    pub(super) key: answers::Key<'id>,
}
impl<'id> Bootstrap<'id> {
    pub fn answer(&self) -> answers::Key<'id> {
        self.key
    }
}

impl<'pool, 'id, 's, I, A, P: Copy + Eq, S, const CAPS: usize, const PATH: usize>
    Driver<'pool, 'id, 's, I, A, P, S, CAPS, PATH>
{
    /// Send Bootstrap and return its root promised capability. The observer
    /// retains the question through pipelined calls and resolution; maintenance
    /// reclaims the response and emits Finish once it is no longer needed.
    /// Admission failure leaves no untracked question or graph reference.
    pub fn bootstrap(
        &mut self,
        messages: &mut messages::Messages<'pool, '_, '_>,
        queue: &mut messages::Queue<'pool, '_, impl Default>,
        minimum: usize,
        options: capnp::message::ReaderOptions,
    ) -> Result<clients::Client<'id>, Error> {
        let question = self.send(messages, queue, minimum, options, calls::bootstrap)?;
        let client = self.pipeline(question, pipeline::Path::new(&[]));
        // The original request remains tracked through a late Return, even if
        // pipeline capacity was exhausted. A live observer prevents early Finish.
        self.cancel(question).expect("newly admitted bootstrap");
        client
    }

    /// Freeze the factory's capability (or failure) exactly once. This needs no
    /// output slot, so pipelines can resolve while the FIFO is full. On failure
    /// the caller still owns its capability and can retry the same token.
    /// Publish with `send_answer`; that operation handles Finish-before-Return.
    pub fn complete_bootstrap(
        &mut self,
        bootstrap: &Bootstrap<'id>,
        capability: Result<&clients::Client<'id>, capnp::ErrorKind>,
        messages: &mut messages::Messages<'pool, '_, '_>,
        minimum: usize,
        options: capnp::message::ReaderOptions,
    ) -> Result<(), Error> {
        self.check_answer_completion(bootstrap.key)?;
        let result = match capability {
            Ok(client) => Ok(self.local_result_dynamic(
                0,
                0,
                [Some(client)].into_iter(),
                messages,
                minimum,
                options,
                |content| content.set_capability_index(0),
            )?),
            Err(error) => Err(error),
        };
        self.complete_local_answer(bootstrap.key, result, messages)
            .map_err(|rejected| {
                if let Ok(result) = rejected.result {
                    result.release(messages, &mut self.runtime);
                }
                rejected.error
            })
    }
}
