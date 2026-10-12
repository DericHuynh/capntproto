//! Terminal faults and cancellation recover owned task reservations.
use super::*;
use capntproto_rpc_engine::{
    io::{IoError, Read},
    messages,
};
use capntproto_rpc_wire::{io::ReceiveError, owned::tasks};
use core::{
    future::Future,
    num::NonZeroUsize,
    task::{Context, Poll, Waker},
};

struct Input<'a> {
    bytes: &'a [u8],
    position: usize,
    mode: u8,
    yielded: bool,
}
impl Read for Input<'_> {
    type Error = u8;
    fn poll_read(
        &mut self,
        cx: &mut Context<'_>,
        destination: &mut [u8],
    ) -> Poll<core::result::Result<usize, IoError<u8>>> {
        if !self.yielded {
            self.yielded = true;
            cx.waker().wake_by_ref();
            return Poll::Pending;
        }
        if self.mode == 2 && self.position == 4 {
            return Poll::Ready(Err(IoError::Failed(7)));
        }
        if self.mode == 3 {
            return Poll::Ready(Ok(destination.len() + 1));
        }
        let count = destination
            .len()
            .min(self.bytes.len() - self.position)
            .min(1);
        destination[..count].copy_from_slice(&self.bytes[self.position..self.position + count]);
        self.position += count;
        Poll::Ready(Ok(count))
    }
}

pub fn owned_task_failures() -> Result<()> {
    for mode in 0..7 {
        let mut data = [0u8; 32];
        data[4] = 1;
        data[20] = 1;
        if mode == 4 {
            data[4] = 10;
        }
        if mode == 5 {
            data[..4].copy_from_slice(&u32::MAX.to_le_bytes());
        }
        let count = match mode {
            1 => 4,
            6 => 0,
            _ => 32,
        };
        let mut input = Input {
            bytes: &data[..count],
            position: 0,
            mode,
            yielded: false,
        };
        let mut bytes = [ZERO; 4];
        let mut buffers = [messages::Buffer::new(Word::words_to_bytes_mut(&mut bytes))];
        messages::with_messages(&mut buffers, &mut [], |messages| {
            let writer = messages.reserve(32).unwrap();
            let writing = messages.write_owned(writer).unwrap();
            let outcome = {
                let mut task = core::pin::pin!(tasks::receive(
                    writing,
                    &mut input,
                    Default::default(),
                    NonZeroUsize::new(2).unwrap()
                ));
                let mut cx = Context::from_waker(Waker::noop());
                assert!(task.as_mut().poll(&mut cx).is_pending());
                let mut completed = None;
                for _ in 0..32 {
                    if let Poll::Ready(result) = task.as_mut().poll(&mut cx) {
                        completed = Some(result);
                        break;
                    }
                }
                completed.expect("receive fault poll bound")
            };
            match (mode, outcome.finish(messages)) {
                (0, Ok(Some(frame))) => {
                    assert_eq!(input.position, 16);
                    frame.release(messages);
                }
                (6, Ok(None)) => assert_eq!(input.position, 0),
                (
                    1,
                    Err(tasks::ReceiveError::Input(ReceiveError::Wire(
                        ErrorKind::PrematureEndOfFile,
                    ))),
                ) => (),
                (2, Err(tasks::ReceiveError::Input(ReceiveError::Io(7)))) => (),
                (
                    3,
                    Err(tasks::ReceiveError::Input(ReceiveError::Wire(
                        ErrorKind::InvalidReadCount(_, _),
                    ))),
                ) => (),
                (
                    4,
                    Err(tasks::ReceiveError::Input(ReceiveError::Wire(
                        ErrorKind::BufferNotLargeEnough,
                    ))),
                ) => (),
                (
                    5,
                    Err(tasks::ReceiveError::Input(ReceiveError::Wire(
                        ErrorKind::InvalidNumberOfSegments(0),
                    ))),
                ) => (),
                _ => panic!("unexpected receive outcome"),
            }
            let writer = messages.reserve(32).unwrap();
            messages.discard(writer);
        })
        .unwrap();
    }
    let mut bytes = [ZERO; 16];
    let mut buffers = [messages::Buffer::new(Word::words_to_bytes_mut(&mut bytes))];
    messages::with_messages(&mut buffers, &mut [], |messages| {
        let writer = messages.reserve(128).unwrap();
        let writing = messages.write_owned(writer).unwrap();
        let result = {
            let mut task =
                core::pin::pin!(tasks::build(writing, Default::default(), async |builder| {
                    builder.try_set_root_text("discarded")?;
                    let mut yielded = false;
                    core::future::poll_fn(|cx| {
                        if yielded {
                            Poll::Ready(())
                        } else {
                            yielded = true;
                            cx.waker().wake_by_ref();
                            Poll::Pending
                        }
                    })
                    .await;
                    Err::<(), _>(capnp::Error::from_kind(ErrorKind::Unimplemented))
                }));
            let mut cx = Context::from_waker(Waker::noop());
            assert!(task.as_mut().poll(&mut cx).is_pending());
            let Poll::Ready(result) = task.as_mut().poll(&mut cx) else {
                panic!("build poll bound")
            };
            result
        };
        assert!(matches!(
            result.finish(messages),
            Err(capntproto_rpc_wire::owned::Error::Wire(
                ErrorKind::Unimplemented
            ))
        ));
        let writer = messages.reserve(128).unwrap();
        let writing = messages.write_owned(writer).unwrap();
        let mut input = Input {
            bytes: &[],
            position: 0,
            mode: 0,
            yielded: false,
        };
        {
            let mut task = core::pin::pin!(tasks::receive(
                writing,
                &mut input,
                Default::default(),
                NonZeroUsize::new(2).unwrap()
            ));
            assert!(task
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop()))
                .is_pending());
            // Dropping an incomplete read is terminal, never a frame boundary.
        }
        assert_eq!(messages.reserve(1).err(), Some(messages::Error::NoBuffer));
    })
    .unwrap();
    messages::with_messages(&mut buffers, &mut [], |messages| {
        let writer = messages.reserve(128).unwrap();
        assert!(messages.write(&writer).iter().all(|&byte| byte == 0));
        messages.discard(writer);
    })
    .unwrap();
    Ok(())
}
