//! Capability-scoped bulk transfer over ordinary RPC with strict payload credit.
//! A write reply acknowledges processing; only done publishes the staged value.
use crate::bulk_capnp as wire;
use capnp::Error;
use futures::{
    future::{LocalBoxFuture, Shared},
    stream::FuturesUnordered,
    FutureExt, StreamExt,
};
use std::{cell::RefCell, rc::Rc};
mod config;
mod credit;
pub use config::Config;
pub use credit::{CreditWindow, Reservation, Settlement};
pub use wire::Status;

fn failed(text: &str) -> Error {
    Error::failed(text.into())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Summary {
    pub bytes: u64,
    pub chunks: u64,
}
struct State {
    config: Config,
    summary: Summary,
    phase: ReceivePhase,
}
enum ReceivePhase {
    Receiving(Vec<u8>),
    Complete(Rc<[u8]>),
    Failed(Error),
    // Cancellation releases staging but preserves the first failure, if any.
    Canceled(Option<Error>),
}
impl State {
    fn status(&self) -> Status {
        match self.phase {
            ReceivePhase::Receiving(_) => Status::Receiving,
            ReceivePhase::Complete(_) => Status::Complete,
            ReceivePhase::Failed(_) => Status::Failed,
            ReceivePhase::Canceled(_) => Status::Canceled,
        }
    }
    fn failure(&self) -> Option<&Error> {
        match &self.phase {
            ReceivePhase::Failed(error) | ReceivePhase::Canceled(Some(error)) => Some(error),
            _ => None,
        }
    }
    fn reject(&mut self, error: Error) -> Error {
        let error = self.failure().cloned().unwrap_or(error);
        self.phase = ReceivePhase::Failed(error.clone());
        error
    }
    fn receiving(&self) -> capnp::Result<()> {
        if let Some(error) = self.failure() {
            return Err(error.clone());
        }
        if !matches!(self.phase, ReceivePhase::Receiving(_)) {
            return Err(failed("bulk transfer is closed"));
        }
        Ok(())
    }
    fn write(&mut self, sequence: u64, bytes: &[u8]) -> capnp::Result<()> {
        self.receiving()?;
        if sequence != self.summary.chunks + 1
            || sequence > u64::from(self.config.max_chunks())
            || bytes.is_empty()
            || bytes.len() > self.config.max_chunk_bytes() as usize
            || self.summary.bytes + bytes.len() as u64 > self.config.length()
        {
            return Err(self.reject(failed("bulk chunk violates sequence or size limits")));
        }
        let ReceivePhase::Receiving(staged) = &mut self.phase else {
            unreachable!("receiving checked above")
        };
        if staged.try_reserve(bytes.len()).is_err() {
            return Err(self.reject(Error::overloaded("bulk allocation failed".into())));
        }
        staged.extend_from_slice(bytes);
        self.summary.bytes += bytes.len() as u64;
        self.summary.chunks += 1;
        Ok(())
    }
    fn done(&mut self) -> capnp::Result<Summary> {
        if matches!(self.phase, ReceivePhase::Complete(_)) {
            return Ok(self.summary);
        }
        self.receiving()?;
        if self.summary.bytes != self.config.length() {
            return Err(self.reject(failed("bulk transfer is incomplete")));
        }
        let ReceivePhase::Receiving(staged) = &mut self.phase else {
            unreachable!("receiving checked above")
        };
        self.phase = ReceivePhase::Complete(std::mem::take(staged).into());
        Ok(self.summary)
    }
    fn cancel(&mut self) -> Status {
        if !matches!(self.phase, ReceivePhase::Complete(_)) {
            let previous = std::mem::replace(&mut self.phase, ReceivePhase::Canceled(None));
            if let ReceivePhase::Failed(error) | ReceivePhase::Canceled(Some(error)) = previous {
                self.phase = ReceivePhase::Canceled(Some(error));
            }
        }
        self.status()
    }
}
struct Owner(Rc<RefCell<State>>);
impl Drop for Owner {
    fn drop(&mut self) {
        self.0.borrow_mut().cancel();
    }
}

/// Application owner of a single transfer. Last owner drop cancels staging.
#[derive(Clone)]
pub struct Receiver(Rc<Owner>);
impl Receiver {
    /// Create an empty receiver from checked limits.
    #[must_use]
    pub fn new(config: Config) -> (Self, wire::transfer::Client) {
        let state = Rc::new(RefCell::new(State {
            config,
            summary: Summary {
                bytes: 0,
                chunks: 0,
            },
            phase: ReceivePhase::Receiving(Vec::new()),
        }));
        let client = capnp_rpc::new_client(Service(state.clone()));
        (Self(Rc::new(Owner(state))), client)
    }
    /// Entry point for an adapter that already possesses transfer authority.
    pub fn write(&self, sequence: u64, bytes: &[u8]) -> capnp::Result<()> {
        self.0 .0.borrow_mut().write(sequence, bytes)
    }
    pub fn done(&self) -> capnp::Result<Summary> {
        self.0 .0.borrow_mut().done()
    }
    pub fn cancel(&self) -> Status {
        self.0 .0.borrow_mut().cancel()
    }
    pub fn status(&self) -> Status {
        self.0 .0.borrow().status()
    }
    pub fn progress(&self) -> Summary {
        self.0 .0.borrow().summary
    }
    pub fn staged_bytes(&self) -> usize {
        match &self.0 .0.borrow().phase {
            ReceivePhase::Receiving(staged) => staged.len(),
            _ => 0,
        }
    }
    pub fn completed(&self) -> Option<Rc<[u8]>> {
        match &self.0 .0.borrow().phase {
            ReceivePhase::Complete(bytes) => Some(bytes.clone()),
            _ => None,
        }
    }
    pub fn failure(&self) -> Option<Error> {
        self.0 .0.borrow().failure().cloned()
    }
    /// Application rejection is sticky and releases all unpublished bytes.
    pub fn fail(&self, error: Error) {
        let mut state = self.0 .0.borrow_mut();
        if state.status() == Status::Receiving {
            state.reject(error);
        }
    }
}
struct Service(Rc<RefCell<State>>);
impl wire::transfer::Server for Service {
    async fn describe(
        self: Rc<Self>,
        _: wire::transfer::DescribeParams,
        mut out: wire::transfer::DescribeResults,
    ) -> capnp::Result<()> {
        self.0.borrow().config.write(out.get().init_config());
        Ok(())
    }
    async fn write(
        self: Rc<Self>,
        p: wire::transfer::WriteParams,
        mut out: wire::transfer::WriteResults,
    ) -> capnp::Result<()> {
        let result = (|| {
            let reader = p.get()?;
            let sequence = reader.get_sequence();
            self.0.borrow_mut().write(sequence, reader.get_data()?)?;
            Ok(sequence)
        })();
        match result {
            Ok(sequence) => {
                out.get().set_sequence(sequence);
                Ok(())
            }
            Err(error) => {
                let mut state = self.0.borrow_mut();
                // Malformed wire pointers poison an open transfer too.
                if state.status() == Status::Receiving {
                    Err(state.reject(error))
                } else {
                    Err(error)
                }
            }
        }
    }
    async fn done(
        self: Rc<Self>,
        _: wire::transfer::DoneParams,
        mut out: wire::transfer::DoneResults,
    ) -> capnp::Result<()> {
        let summary = self.0.borrow_mut().done()?;
        let mut r = out.get().init_summary();
        r.set_bytes(summary.bytes);
        r.set_chunks(summary.chunks);
        Ok(())
    }
    async fn cancel(
        self: Rc<Self>,
        _: wire::transfer::CancelParams,
        mut out: wire::transfer::CancelResults,
    ) -> capnp::Result<()> {
        out.get().set_status(self.0.borrow_mut().cancel());
        Ok(())
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum Phase {
    Sending,
    Finishing,
    Canceling,
    Finished,
    Canceled,
}
type Completion<T> = Shared<LocalBoxFuture<'static, capnp::Result<T>>>;
type Ack = LocalBoxFuture<'static, (Reservation, capnp::Result<()>)>;

/// One ordered producer per fresh transfer capability. Borrowing mutably
/// serializes admission; other holders can cancel or invalidate the transfer.
pub struct Sender {
    client: wire::transfer::Client,
    config: Config,
    window: CreditWindow,
    sent_bytes: u64,
    pending: FuturesUnordered<Ack>,
    error: Option<Error>,
    phase: Phase,
    finish: Option<Completion<Summary>>,
    cancellation: Option<Completion<Status>>,
}
impl Sender {
    pub async fn connect(client: wire::transfer::Client) -> capnp::Result<Self> {
        let response = client.describe_request().send().promise.await?;
        let config = Config::read(response.get()?.get_config()?)?;
        Ok(Self {
            window: CreditWindow::new(config.window_bytes(), config.max_chunks())?,
            client,
            config,
            sent_bytes: 0,
            pending: FuturesUnordered::new(),
            error: None,
            phase: Phase::Sending,
            finish: None,
            cancellation: None,
        })
    }
    pub fn config(&self) -> &Config {
        &self.config
    }
    pub fn in_flight(&self) -> u32 {
        self.window.in_flight()
    }
    pub fn sent_bytes(&self) -> u64 {
        self.sent_bytes
    }
    fn remember(&mut self, error: Error) {
        if self.error.is_none() {
            self.error = Some(error);
        }
    }
    async fn receive_one(&mut self) {
        if let Some((reservation, result)) = self.pending.next().await {
            // Both success and error Returns settle this call. An error is
            // sticky, so released credit cannot authorize subsequent writes.
            if let Err(error) = self.window.settle(&reservation).and(result) {
                self.remember(error);
            }
        }
    }
    /// Wait for byte credit, then send. Success means admission, not execution.
    /// Canceling this future while it waits does not consume credit or a sequence.
    pub async fn write(&mut self, bytes: &[u8]) -> capnp::Result<()> {
        if self.phase != Phase::Sending {
            return Err(failed("bulk sender closed"));
        }
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        if bytes.is_empty()
            || bytes.len() > self.config.max_chunk_bytes() as usize
            || self.sent_bytes + bytes.len() as u64 > self.config.length()
            || self.window.issued() >= u64::from(self.config.max_chunks())
        {
            return Err(failed("bulk write outside negotiated limits"));
        }
        let bytes_len = bytes.len() as u32;
        while self.window.available() < bytes_len {
            self.receive_one().await;
            if let Some(error) = &self.error {
                return Err(error.clone());
            }
        }
        let reservation = self.window.reserve(bytes_len)?.expect("credit checked");
        let sequence = reservation.sequence();
        self.sent_bytes += u64::from(bytes_len);
        let mut request = self.client.write_request();
        request.get().set_sequence(sequence);
        request.get().set_data(bytes);
        let reply = request.send().promise;
        self.pending.push(
            async move {
                let result = async {
                    if reply.await?.get()?.get_sequence() != sequence {
                        return Err(failed("mismatched bulk acknowledgment"));
                    }
                    Ok(())
                }
                .await;
                (reservation, result)
            }
            .boxed_local(),
        );
        Ok(())
    }
    /// Observe all outstanding replies, retaining the first error. Dropping
    /// this wait leaves its pending RPCs and their byte reservations owned here.
    pub async fn flush(&mut self) -> capnp::Result<()> {
        while !self.pending.is_empty() {
            self.receive_one().await;
        }
        self.error.clone().map_or(Ok(()), Err)
    }
    /// Seal submission and publish remotely. The retained completion survives
    /// cancellation of this wait; a subsequent done() resumes observation.
    pub async fn done(&mut self) -> capnp::Result<Summary> {
        if matches!(self.phase, Phase::Canceling | Phase::Canceled) {
            return Err(failed("bulk sender canceled"));
        }
        if self.finish.is_none() {
            self.phase = Phase::Finishing;
            let reply = self.client.done_request().send().promise;
            let expected = Summary {
                bytes: self.config.length(),
                chunks: self.window.issued(),
            };
            self.finish = Some(
                async move {
                    let r = reply.await?;
                    let r = r.get()?.get_summary()?;
                    let summary = Summary {
                        bytes: r.get_bytes(),
                        chunks: r.get_chunks(),
                    };
                    if summary != expected {
                        return Err(failed("mismatched bulk completion"));
                    }
                    Ok(summary)
                }
                .boxed_local()
                .shared(),
            );
        }
        let _ = self.flush().await;
        let result = self.finish.as_ref().unwrap().clone().await;
        self.phase = Phase::Finished;
        if let Err(error) = &result {
            self.remember(error.clone());
        }
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        result
    }
    /// Ordered application cancellation, not RPC Finish cancellation. Earlier
    /// calls may execute; Complete means publication already won the race.
    pub async fn cancel(&mut self) -> capnp::Result<Status> {
        if self.cancellation.is_none() {
            self.phase = Phase::Canceling;
            let reply = self.client.cancel_request().send().promise;
            self.cancellation = Some(
                async move {
                    let status = reply.await?.get()?.get_status()?;
                    if !matches!(status, Status::Canceled | Status::Complete) {
                        return Err(failed("invalid bulk cancellation status"));
                    }
                    Ok(status)
                }
                .boxed_local()
                .shared(),
            );
        }
        let _ = self.flush().await;
        let result = self.cancellation.as_ref().unwrap().clone().await;
        self.phase = if matches!(result, Ok(Status::Complete)) {
            Phase::Finished
        } else {
            Phase::Canceled
        };
        result
    }
}
