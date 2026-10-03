//! The library enables the nightly trait; callers only use ordinary `Result?`.
use capnp::{capability::Promise, Error, ErrorKind};
use std::{
    cell::Cell,
    future::Future,
    marker::PhantomPinned,
    pin::Pin,
    rc::Rc,
    task::{Context, Poll, Waker},
};

fn poll<T>(promise: &mut Promise<T, Error>) -> Poll<capnp::Result<T>> {
    Pin::new(promise).poll(&mut Context::from_waker(Waker::noop()))
}

fn ready<T>(mut promise: Promise<T, Error>) -> capnp::Result<T> {
    match poll(&mut promise) {
        Poll::Ready(result) => result,
        Poll::Pending => panic!("expected immediate completion"),
    }
}

#[derive(Default)]
struct Activity {
    polls: Cell<usize>,
    drops: Cell<usize>,
}

struct PendingOnce {
    activity: Rc<Activity>,
    result: Option<capnp::Result<usize>>,
}

impl Future for PendingOnce {
    type Output = capnp::Result<usize>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let polls = self.activity.polls.get();
        self.activity.polls.set(polls + 1);
        if polls == 0 {
            cx.waker().wake_by_ref();
            Poll::Pending
        } else {
            Poll::Ready(self.result.take().expect("polled after completion"))
        }
    }
}

impl Drop for PendingOnce {
    fn drop(&mut self) {
        self.activity.drops.set(self.activity.drops.get() + 1);
    }
}

fn deferred(activity: &Rc<Activity>, result: capnp::Result<usize>) -> Promise<usize, Error> {
    Promise::from_future(PendingOnce {
        activity: activity.clone(),
        result: Some(result),
    })
}

fn validate_then_return(
    validation: capnp::Result<()>,
    promise: Promise<usize, Error>,
    continued: &Cell<bool>,
) -> Promise<usize, Error> {
    validation?;
    continued.set(true);
    promise
}

#[test]
fn success_continues_with_borrowed_non_unpin_output() {
    struct Value<'a>(&'a str, PhantomPinned);
    fn validate<'a>(text: &'a str, continued: &Cell<bool>) -> Promise<Value<'a>, Error> {
        let value = Ok::<_, Error>(Value(text, PhantomPinned))?;
        continued.set(true);
        Promise::ok(value)
    }
    let text = String::from("borrowed");
    let continued = Cell::new(false);
    let promise = validate(&text, &continued);
    assert!(continued.get());
    assert_eq!(ready(promise).unwrap().0.as_ptr(), text.as_ptr());
}

#[test]
fn error_short_circuits_and_moves_all_exception_metadata() {
    let mut error = Error::disconnected("remote error".into());
    error.set_remote_trace("remote trace".into());
    error.set_detail(42, vec![1, 2, 3]);
    let extra = error.extra.as_ptr();
    let trace = error.remote_trace().unwrap().as_ptr();
    let detail = error.detail(42).unwrap().as_ptr();
    let activity = Rc::default();
    let continued = Cell::new(false);
    let promise = validate_then_return(Err(error), deferred(&activity, Ok(7)), &continued);
    assert!(!continued.get());
    let error = ready(promise).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Disconnected);
    assert_eq!(error.extra, "remote error");
    assert_eq!(error.extra.as_ptr(), extra);
    assert_eq!(error.remote_trace(), Some("remote trace"));
    assert_eq!(error.remote_trace().unwrap().as_ptr(), trace);
    assert_eq!(error.detail(42), Some([1, 2, 3].as_slice()));
    assert_eq!(error.detail(42).unwrap().as_ptr(), detail);
    assert_eq!(activity.polls.get(), 0);
    assert_eq!(activity.drops.get(), 1);
}

#[test]
fn convertible_error_is_converted_exactly_once() {
    struct ValidationError(Rc<Cell<usize>>);
    impl From<ValidationError> for Error {
        fn from(error: ValidationError) -> Self {
            error.0.set(error.0.get() + 1);
            Self::failed("converted".into())
        }
    }
    fn validate(error: ValidationError) -> Promise<(), Error> {
        Err::<(), _>(error)?;
        panic!("validation must return early");
    }
    let conversions = Rc::new(Cell::new(0));
    let promise = validate(ValidationError(conversions.clone()));
    assert_eq!(conversions.get(), 1);
    assert_eq!(ready(promise).unwrap_err().extra, "converted");
    assert_eq!(conversions.get(), 1);
}

#[test]
fn utf8_validation_supports_existing_error_conversion() {
    fn validate(bytes: &[u8]) -> Promise<&str, Error> {
        let text = core::str::from_utf8(bytes)?;
        Promise::ok(text)
    }
    assert_eq!(ready(validate(b"valid")).unwrap(), "valid");
    let actual = ready(validate(&[0xff])).unwrap_err();
    let ErrorKind::TextContainsNonUtf8Data(error) = actual.kind else {
        panic!("wrong validation error: {actual}");
    };
    assert_eq!(error.valid_up_to(), 0);
    assert_eq!(error.error_len(), Some(1));
}

#[test]
fn successful_validation_does_not_poll_deferred_work() {
    let activity = Rc::default();
    let continued = Cell::new(false);
    let mut promise = validate_then_return(Ok(()), deferred(&activity, Ok(7)), &continued);
    assert!(continued.get());
    assert_eq!(activity.polls.get(), 0);
    assert_eq!(activity.drops.get(), 0);
    assert!(poll(&mut promise).is_pending());
    assert_eq!(activity.polls.get(), 1);
    assert_eq!(activity.drops.get(), 0);
    assert_eq!(ready(promise).unwrap(), 7);
    assert_eq!(activity.polls.get(), 2);
    assert_eq!(activity.drops.get(), 1);
}

#[test]
fn dropping_returned_promise_cancels_unpolled_work() {
    let activity = Rc::default();
    let promise = validate_then_return(Ok(()), deferred(&activity, Ok(7)), &Cell::new(false));
    drop(promise);
    assert_eq!(activity.polls.get(), 0);
    assert_eq!(activity.drops.get(), 1);
}

#[test]
fn early_error_cancels_already_pending_work() {
    let activity = Rc::default();
    let mut work = deferred(&activity, Ok(7));
    assert!(poll(&mut work).is_pending());
    let promise = validate_then_return(
        Err(Error::failed("validation".into())),
        work,
        &Cell::new(false),
    );
    assert_eq!(activity.polls.get(), 1);
    assert_eq!(activity.drops.get(), 1);
    assert_eq!(ready(promise).unwrap_err().extra, "validation");
}

#[test]
fn await_question_mark_propagates_deferred_errors() {
    let activity = Rc::default();
    let continued = Rc::new(Cell::new(false));
    let after_await = continued.clone();
    let work = deferred(&activity, Err(Error::failed("asynchronous".into())));
    let mut promise = Promise::from_future(async move {
        let value = work.await?;
        after_await.set(true);
        Ok(value)
    });
    assert_eq!(activity.polls.get(), 0);
    assert!(poll(&mut promise).is_pending());
    assert_eq!(ready(promise).unwrap_err().extra, "asynchronous");
    assert!(!continued.get());
    assert_eq!(activity.polls.get(), 2);
    assert_eq!(activity.drops.get(), 1);
}
