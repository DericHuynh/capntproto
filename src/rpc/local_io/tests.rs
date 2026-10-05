use super::*;
use futures::{task::noop_waker_ref, FutureExt};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[derive(Default)]
struct Wakes(AtomicUsize);
impl std::task::Wake for Wakes {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.fetch_add(1, Ordering::Relaxed);
    }
}

#[test]
fn vectored_short_writes_wrap_and_wake_only_the_current_waiter() {
    let (a, b) = pair(5);
    let (mut read, _write) = a.into_split();
    let (_read, mut write) = b.into_split();
    let old = Arc::new(Wakes::default());
    let current = Arc::new(Wakes::default());
    let old_waker = Waker::from(old.clone());
    let current_waker = Waker::from(current.clone());
    let mut buffer = [0; 8];
    for waker in [&old_waker, &current_waker] {
        assert!(Pin::new(&mut read)
            .poll_read(
                &mut Context::from_waker(waker),
                &mut ReadBuf::new(&mut buffer)
            )
            .is_pending());
    }
    let mut cx = Context::from_waker(&current_waker);
    assert!(matches!(
        Pin::new(&mut write).poll_write_vectored(
            &mut cx,
            &[
                io::IoSlice::new(b""),
                io::IoSlice::new(b"abc"),
                io::IoSlice::new(b"defg")
            ]
        ),
        Poll::Ready(Ok(5))
    ));
    assert_eq!(old.0.load(Ordering::Relaxed), 0);
    assert_eq!(current.0.load(Ordering::Relaxed), 1);
    assert!(Pin::new(&mut write)
        .poll_write(&mut cx, b"fgh")
        .is_pending());
    assert_eq!(
        read.read(&mut buffer[..3]).now_or_never().unwrap().unwrap(),
        3
    );
    assert_eq!(&buffer[..3], b"abc");
    assert_eq!(current.0.load(Ordering::Relaxed), 2);
    assert_eq!(write.write(b"fgh").now_or_never().unwrap().unwrap(), 3);
    assert_eq!(read.read(&mut buffer).now_or_never().unwrap().unwrap(), 5);
    assert_eq!(&buffer[..5], b"defgh");
    assert!(read.read(&mut buffer).now_or_never().is_none());
}

#[tokio::test]
async fn half_close_drains_all_bytes_and_keeps_reverse_direction_open() {
    let (mut a, mut b) = pair(3);
    a.write_all(b"abc").await.unwrap();
    a.shutdown().await.unwrap();
    a.shutdown().await.unwrap();
    assert_eq!(
        a.write(b"x").await.unwrap_err().kind(),
        io::ErrorKind::BrokenPipe
    );
    b.write_all(b"xy").await.unwrap();
    let mut bytes = Vec::new();
    b.read_to_end(&mut bytes).await.unwrap();
    assert_eq!(bytes, b"abc");
    drop(b);
    bytes.clear();
    a.read_to_end(&mut bytes).await.unwrap();
    assert_eq!(bytes, b"xy");
}

#[tokio::test]
async fn continuously_ready_io_yields_to_other_tasks() {
    let (mut a, mut b) = pair(1);
    let observed = std::cell::Cell::new(false);
    tokio::join!(
        biased;
        async {
            for _ in 0..512 {
                a.write_all(b"x").await.unwrap();
                b.read_exact(&mut [0]).await.unwrap();
            }
            assert!(observed.get(), "ready IO exhausted no cooperative budget");
        },
        async { observed.set(true); }
    );
}

#[test]
fn cancellation_preserves_unconsumed_bytes_and_peer_drop_wakes_blocked_writes() {
    let (mut a, mut b) = pair(2);
    assert!(a.read(&mut [0; 1]).now_or_never().is_none());
    b.write_all(b"ab").now_or_never().unwrap().unwrap();
    assert!(b.write_all(b"c").now_or_never().is_none());
    let mut bytes = [0; 2];
    a.read_exact(&mut bytes).now_or_never().unwrap().unwrap();
    assert_eq!(&bytes, b"ab");
    b.write_all(b"cd").now_or_never().unwrap().unwrap();
    let wakes = Arc::new(Wakes::default());
    let waker = Waker::from(wakes.clone());
    assert!(Pin::new(&mut b)
        .poll_write(&mut Context::from_waker(&waker), b"e")
        .is_pending());
    drop(a);
    assert_eq!(wakes.0.load(Ordering::Relaxed), 1);
    assert_eq!(
        b.write(b"e").now_or_never().unwrap().unwrap_err().kind(),
        io::ErrorKind::BrokenPipe
    );
    assert!(b.write.0.borrow().bytes.is_empty());
    assert_eq!(b.write.0.borrow().bytes.capacity(), 0);
}

thread_local! {
    static ON_WAKE: RefCell<Option<Box<dyn Fn()>>> = RefCell::new(None);
}
struct Reentrant;
impl std::task::Wake for Reentrant {
    fn wake(self: Arc<Self>) {
        ON_WAKE.with(|hook| (hook.borrow().as_ref().unwrap())());
    }
}

#[test]
fn waking_and_closing_release_the_pipe_borrow_before_callbacks() {
    let (mut a, mut b) = pair(1);
    let pipe = a.read.0.clone();
    ON_WAKE.with(|hook| {
        *hook.borrow_mut() = Some(Box::new(move || {
            assert!(pipe.try_borrow_mut().is_ok());
        }))
    });
    let waker = Waker::from(Arc::new(Reentrant));
    let mut cx = Context::from_waker(&waker);
    let mut byte = [0];
    assert!(Pin::new(&mut a)
        .poll_read(&mut cx, &mut ReadBuf::new(&mut byte))
        .is_pending());
    b.write_all(b"a").now_or_never().unwrap().unwrap();
    assert!(Pin::new(&mut b).poll_write(&mut cx, b"b").is_pending());
    a.read_exact(&mut byte).now_or_never().unwrap().unwrap();
    assert!(Pin::new(&mut a)
        .poll_read(&mut cx, &mut ReadBuf::new(&mut byte))
        .is_pending());
    drop(b);
    ON_WAKE.with(|hook| hook.borrow_mut().take());
}

#[derive(Debug, PartialEq)]
enum Observation {
    Pending,
    Read(Vec<u8>),
    Written(usize),
    Closed,
    Error(io::ErrorKind),
}
fn operation<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    kind: u8,
    bytes: &[u8],
) -> Observation {
    let mut cx = Context::from_waker(noop_waker_ref());
    if kind == 0 {
        let mut bytes = vec![0; bytes.len()];
        let mut output = ReadBuf::new(&mut bytes);
        return match Pin::new(stream).poll_read(&mut cx, &mut output) {
            Poll::Pending => Observation::Pending,
            Poll::Ready(Ok(())) => Observation::Read(output.filled().to_vec()),
            Poll::Ready(Err(e)) => Observation::Error(e.kind()),
        };
    }
    let result = if kind == 1 {
        Pin::new(stream).poll_write(&mut cx, bytes)
    } else if kind == 2 {
        let middle = bytes.len() / 2;
        Pin::new(stream).poll_write_vectored(
            &mut cx,
            &[
                io::IoSlice::new(&bytes[..middle]),
                io::IoSlice::new(&bytes[middle..]),
            ],
        )
    } else {
        return match Pin::new(stream).poll_shutdown(&mut cx) {
            Poll::Pending => Observation::Pending,
            Poll::Ready(Ok(())) => Observation::Closed,
            Poll::Ready(Err(e)) => Observation::Error(e.kind()),
        };
    };
    match result {
        Poll::Pending => Observation::Pending,
        Poll::Ready(Ok(n)) => Observation::Written(n),
        Poll::Ready(Err(e)) => Observation::Error(e.kind()),
    }
}

proptest::proptest! {
    #[test]
    fn matches_duplex_byte_order_backpressure_and_half_closes(
        capacity in 1usize..33,
        actions in proptest::collection::vec((0usize..2, 0u8..4, proptest::collection::vec(proptest::prelude::any::<u8>(), 0..48)), 1..200),
    ) {
        let (a, b) = pair(capacity);
        let mut local = [a, b];
        let (a, b) = tokio::io::duplex(capacity);
        let mut reference = [a, b];
        for (side, kind, bytes) in actions {
            proptest::prop_assert_eq!(operation(&mut local[side], kind, &bytes), operation(&mut reference[side], kind, &bytes));
        }
    }
}
