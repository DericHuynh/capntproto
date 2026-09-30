//! Explicit revocation of locally implemented capability servers.
use capnp::capability::{CallExecutor, FromClientHook, FromServer, Promise, Server};
use capnp::Error;
use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::rc::{Rc, Weak};
use std::task::{Context, Poll, Waker};

/// A revocation owner for all clients issued through this object.
///
/// Revocation cancels pending application futures, including non-cancellable
/// methods, and makes subsequent calls fail. Dropping this owner revokes it.
/// Unlike C++'s borrowed server API, Rust retains an `Rc` until revocation, so
/// no asynchronous call can outlive a borrowed server. Constructing another client
/// for the same server while a revocable client exists returns a
/// broken capability, preventing an alias from bypassing this revocation gate.
/// Capabilities already returned by completed calls are not recursively revoked.
pub struct RevocableServer<C: FromClientHook> {
    client: C,
    revoker: crate::local::Revoker,
}
impl<C: FromClientHook> RevocableServer<C> {
    pub fn new<S>(server: S) -> Self
    where
        C: FromServer<S>,
    {
        Self::from_rc(Rc::new(server))
    }
    pub fn from_rc<S>(server: Rc<S>) -> Self
    where
        C: FromServer<S>,
    {
        Self::from_local(crate::local::Client::new(C::from_server(server)))
    }
    /// Supply an owner for non-cancellable direct local calls. Incoming RPC
    /// calls can instead use their RpcSystem's executor automatically.
    pub fn new_with_executor<S>(server: S, executor: Rc<dyn CallExecutor>) -> Self
    where
        C: FromServer<S>,
    {
        Self::from_rc_with_executor(Rc::new(server), executor)
    }
    pub fn from_rc_with_executor<S>(server: Rc<S>, executor: Rc<dyn CallExecutor>) -> Self
    where
        C: FromServer<S>,
    {
        Self::from_local(crate::local::Client::new_with_executor(
            C::from_server(server),
            executor,
        ))
    }
    /// Associate an owned descriptor with this revocation owner. Revocation
    /// stops new local descriptor lookups, but cannot recall descriptors that
    /// have already been obtained or transferred to another process.
    #[cfg(unix)]
    pub fn new_with_fd<S>(server: S, fd: std::os::fd::OwnedFd) -> Self
    where
        C: FromServer<S>,
    {
        Self::from_local(crate::local::Client::new_with_fd(
            C::from_server(Rc::new(server)),
            Rc::new(fd),
        ))
    }
    /// Descriptor-backed variant with a driver for protected local calls.
    #[cfg(unix)]
    pub fn new_with_fd_and_executor<S>(
        server: S,
        fd: std::os::fd::OwnedFd,
        executor: Rc<dyn CallExecutor>,
    ) -> Self
    where
        C: FromServer<S>,
    {
        Self::from_local(
            crate::local::Client::new_with_fd(C::from_server(Rc::new(server)), Rc::new(fd))
                .with_executor(executor),
        )
    }
    fn from_local<S: Server + Clone + 'static>(client: crate::local::Client<S>) -> Self {
        let (client, revoker) = client.into_revocable();
        Self {
            client: C::new(client.into_hook()),
            revoker,
        }
    }
    pub fn get_client(&self) -> C {
        C::new(self.client.as_client_hook().add_ref())
    }
    /// Includes references held by outstanding requests and protected tasks.
    pub fn is_in_use(&self) -> bool {
        self.revoker.is_in_use()
    }
    pub fn revoke(&self) {
        self.revoke_with_error(default_error());
    }
    /// The first revocation reason wins. Revocation is idempotent.
    pub fn revoke_with_error(&self, error: Error) {
        self.revoker.revoke(error);
    }
}
impl<C: FromClientHook> Drop for RevocableServer<C> {
    fn drop(&mut self) {
        self.revoke();
    }
}
fn default_error() -> Error {
    Error::failed("capability was revoked (RevocableServer was destroyed)".into())
}

#[derive(Default)]
pub(crate) struct Canceler {
    error: RefCell<Option<Error>>,
    tasks: RefCell<Vec<Weak<RefCell<Task>>>>,
}
impl Canceler {
    pub(crate) fn wrap(&self, future: Promise<(), Error>) -> Promise<(), Error> {
        let error = self.error.borrow().clone();
        if let Some(error) = error {
            return Promise::err(error);
        }
        let task = Rc::new(RefCell::new(Task {
            future: Some(future),
            error: None,
            waker: None,
        }));
        let mut tasks = self.tasks.borrow_mut();
        tasks.retain(|task| task.strong_count() != 0);
        tasks.push(Rc::downgrade(&task));
        Promise::from_future(RevocableTask(task))
    }
    pub(crate) fn cancel(&self, error: Error) {
        if self.error.borrow().is_some() {
            return;
        }
        *self.error.borrow_mut() = Some(error.clone());
        let tasks = std::mem::take(&mut *self.tasks.borrow_mut());
        for task in tasks.into_iter().filter_map(|t| t.upgrade()) {
            let (future, waker) = {
                let mut task = task.borrow_mut();
                task.error = Some(error.clone());
                (task.future.take(), task.waker.take())
            };
            // Application destructors and custom wakers may re-enter revocation.
            drop(future);
            if let Some(waker) = waker {
                waker.wake();
            }
        }
    }
}
struct Task {
    future: Option<Promise<(), Error>>,
    error: Option<Error>,
    waker: Option<Waker>,
}
struct RevocableTask(Rc<RefCell<Task>>);
impl Future for RevocableTask {
    type Output = Result<(), Error>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut future = {
            let mut task = self.0.borrow_mut();
            if let Some(error) = &task.error {
                return Poll::Ready(Err(error.clone()));
            }
            task.waker = Some(cx.waker().clone());
            task.future
                .take()
                .expect("revocable task polled after completion")
        };
        // Poll without a RefCell borrow: a method can revoke its own capability.
        let result = Pin::new(&mut future).poll(cx);
        let error = self.0.borrow().error.clone();
        if let Some(error) = error {
            // A currently executing Rust poll cannot be preempted. Self-revocation
            // destroys its future as soon as that poll returns, before completion
            // is delivered to a caller.
            drop(future);
            return Poll::Ready(Err(error));
        }
        if result.is_pending() {
            self.0.borrow_mut().future = Some(future);
        } else {
            self.0.borrow_mut().waker = None;
        }
        result
    }
}
