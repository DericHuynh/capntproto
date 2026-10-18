//! Real C++ TCP interoperability using the caller-backed RPC driver.
#[path = "support/bounded_driver.rs"]
mod resources;
use capnp_rpc::{
    bounded::{
        calls,
        owned::{
            self,
            connection::{Error, Progress, Response, Termination},
            tasks, Frame,
        },
        pipeline,
    },
    engine::{
        clients,
        io::{self, IoError},
        messages, questions,
    },
};
use capntproto_test_support::bounded::runtime_test_capnp::{
    api::harness::{BounceParams, EchoParams},
    harness,
};
use std::{
    future::Future,
    io::{BufRead, BufReader, Read, Write},
    net::TcpStream,
    num::NonZeroUsize,
    process::{Child, Command, Stdio},
    task::{Context, Poll, Waker},
    time::Duration,
};
const BYTES: usize = 4096;
fn budget() -> NonZeroUsize {
    NonZeroUsize::new(2).unwrap()
}
struct PeerProcess(Child);
impl Drop for PeerProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
/// Blocking socket IO is chosen only by this test adapter. Short reads/writes
/// exercise the same caller-polled framing contract as an embedded transport.
struct Socket(TcpStream);
impl io::Read for Socket {
    type Error = std::io::Error;
    fn poll_read(
        &mut self,
        _: &mut Context<'_>,
        bytes: &mut [u8],
    ) -> Poll<Result<usize, IoError<Self::Error>>> {
        let n = bytes.len().min(7);
        Poll::Ready(self.0.read(&mut bytes[..n]).map_err(|e| {
            if e.kind() == std::io::ErrorKind::Interrupted {
                IoError::Interrupted
            } else {
                IoError::Failed(e)
            }
        }))
    }
}
impl io::Flush for Socket {
    type Error = std::io::Error;
    fn poll_flush(&mut self, _: &mut Context<'_>) -> Poll<Result<(), IoError<Self::Error>>> {
        // std TCP writes go directly to the socket, with no userspace buffer.
        Poll::Ready(Ok(()))
    }
}
impl io::Write for Socket {
    fn poll_write(
        &mut self,
        _: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<Result<usize, IoError<Self::Error>>> {
        Poll::Ready(self.0.write(&bytes[..bytes.len().min(11)]).map_err(|e| {
            if e.kind() == std::io::ErrorKind::Interrupted {
                IoError::Interrupted
            } else {
                IoError::Failed(e)
            }
        }))
    }
}
fn flush<'p>(
    io: &mut Socket,
    messages: &mut messages::Messages<'p, '_, '_>,
    queue: &mut messages::Queue<'p, '_>,
) {
    let mut cx = Context::from_waker(Waker::noop());
    for _ in 0..65536 {
        if let Poll::Ready(result) = io::poll_queue(io, queue, messages, &mut cx, budget()) {
            result.unwrap();
            return;
        }
    }
    panic!("output did not drain");
}
fn receive<'p>(io: &mut Socket, messages: &mut messages::Messages<'p, '_, '_>) -> Frame<'p> {
    let receiving = messages.receive(BYTES).unwrap();
    let mut future = core::pin::pin!(tasks::receive(receiving, io, Default::default(), budget()));
    let mut cx = Context::from_waker(Waker::noop());
    for _ in 0..65536 {
        if let Poll::Ready(result) = future.as_mut().poll(&mut cx) {
            return result.finish(messages).unwrap().expect("C++ EOF");
        }
    }
    panic!("input did not complete");
}
fn maintain<'p>(
    driver: &mut resources::Peer<'p, '_, '_>,
    io: &mut Socket,
    messages: &mut messages::Messages<'p, '_, '_>,
    queue: &mut messages::Queue<'p, '_>,
) {
    for _ in 0..4096 {
        flush(io, messages, queue);
        match driver
            .poll_maintenance(
                messages,
                queue,
                BYTES,
                Default::default(),
                &mut [0; 4],
                budget(),
            )
            .unwrap()
        {
            Progress::Idle => {
                flush(io, messages, queue);
                return;
            }
            Progress::Yield | Progress::Advanced | Progress::Import(()) => (),
            _ => panic!("client-only connection produced a server owner"),
        }
    }
    panic!("maintenance did not quiesce");
}
fn response<'p, 'id>(
    driver: &mut resources::Peer<'p, 'id, '_>,
    question: questions::Key<'id>,
    io: &mut Socket,
    messages: &mut messages::Messages<'p, '_, '_>,
    queue: &mut messages::Queue<'p, '_>,
) -> Response<'p, 'id, (), 4> {
    for _ in 0..100 {
        maintain(driver, io, messages, queue);
        match driver.take_response(question) {
            Ok(response) => return response,
            Err(Error::PendingResponse) => (),
            Err(error) => panic!("response: {error:?}"),
        }
        let frame = receive(io, messages);
        driver
            .receive(frame, messages, &mut [0; 4], |_| (), || ())
            .unwrap_or_else(|r| panic!("C++ input rejected: {:?}", r.error));
    }
    panic!("C++ response did not arrive");
}
fn call<'p, 'id>(
    driver: &mut resources::Peer<'p, 'id, '_>,
    target: &clients::Client<'id>,
    value: u32,
    io: &mut Socket,
    messages: &mut messages::Messages<'p, '_, '_>,
    queue: &mut messages::Queue<'p, '_>,
) -> questions::Key<'id> {
    driver
        .enqueue_call(
            target,
            harness::Owned::ECHO,
            &[],
            messages,
            BYTES,
            Default::default(),
            |mut p| {
                p.scalar(EchoParams::VALUE).set(value);
                Ok(())
            },
        )
        .unwrap();
    for _ in 0..100 {
        maintain(driver, io, messages, queue);
        if let Some(sent) = driver
            .send_invocation(messages, queue, BYTES, Default::default(), &mut [0; 4])
            .unwrap()
        {
            return sent.question;
        }
        let frame = receive(io, messages);
        driver
            .receive(frame, messages, &mut [0; 4], |_| (), || ())
            .unwrap_or_else(|r| panic!("call routing input: {:?}", r.error));
    }
    panic!("call did not become ready");
}
fn main() {
    let executable = std::env::args().nth(1).expect("C++ peer executable");
    let mut process = PeerProcess(
        Command::new(executable)
            .args(std::env::args().skip(2))
            .stdout(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let mut port = String::new();
    BufReader::new(process.0.stdout.take().unwrap())
        .read_line(&mut port)
        .unwrap();
    let mut io =
        Socket(TcpStream::connect(("127.0.0.1", port.trim().parse::<u16>().unwrap())).unwrap());
    io.0.set_nodelay(true).unwrap();
    io.0.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    io.0.set_write_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut words = [[capnp::word(0, 0, 0, 0, 0, 0, 0, 0); BYTES / 8]; 12];
    let mut buffers = words
        .each_mut()
        .map(|words| messages::Buffer::new(capnp::Word::words_to_bytes_mut(words)));
    let allocations = allocation_counter::measure(|| {
        messages::with_messages(
            &mut buffers,
            &mut [const { messages::SendSlot::EMPTY }; 2],
            |messages| {
                let mut slots = [None, None];
                let mut queue = messages::Queue::new(&mut slots).unwrap();
                resources::with_owned_peer_tasks::<4, _>(|mut driver| {
                    let bootstrap = driver
                        .send(
                            messages,
                            &mut queue,
                            BYTES,
                            Default::default(),
                            calls::bootstrap,
                        )
                        .unwrap();
                    let target = driver
                        .pipeline(bootstrap, pipeline::Path::new(&[]))
                        .unwrap();
                    // Send before receiving bootstrap, exercising the promised-answer target.
                    let question = call(&mut driver, &target, 17, &mut io, messages, &mut queue);
                    let reply = response(&mut driver, bootstrap, &mut io, messages, &mut queue);
                    driver.release_response(reply, messages);
                    let reply = response(&mut driver, question, &mut io, messages, &mut queue);
                    reply
                        .with(messages, |reply| {
                            let owned::Reply::Results(payload) = reply else {
                                panic!("echo result")
                            };
                            assert_eq!(
                                harness::Owned::ECHO
                                    .read_results(payload.content())
                                    .unwrap()
                                    .value(),
                                117
                            );
                        })
                        .unwrap();
                    driver.release_response(reply, messages);
                    let question = call(&mut driver, &target, 1001, &mut io, messages, &mut queue);
                    let reply = response(&mut driver, question, &mut io, messages, &mut queue);
                    reply
                        .with(messages, |reply| {
                            let owned::Reply::Exception(error) = reply else {
                                panic!("C++ overload exception")
                            };
                            assert_eq!(
                                error.get_type().unwrap(),
                                capnp_rpc::rpc_capnp::exception::Type::Overloaded
                            );
                            assert!(error
                                .get_reason()
                                .unwrap()
                                .to_str()
                                .unwrap()
                                .contains("cpp failure"));
                        })
                        .unwrap();
                    driver.release_response(reply, messages);
                    maintain(&mut driver, &mut io, messages, &mut queue);
                    let task = driver
                        .enqueue_call(
                            &target,
                            harness::Owned::PENDING,
                            &[],
                            messages,
                            BYTES,
                            Default::default(),
                            |_| Ok(()),
                        )
                        .unwrap();
                    let child = driver
                        .invocation_pipeline(task, pipeline::Path::new(&[0]), messages)
                        .unwrap();
                    let pending = driver
                        .send_invocation(
                            messages,
                            &mut queue,
                            BYTES,
                            Default::default(),
                            &mut [0; 4],
                        )
                        .unwrap()
                        .unwrap()
                        .question;
                    let pipelined = call(&mut driver, &child, 23, &mut io, messages, &mut queue);
                    let reply = response(&mut driver, pending, &mut io, messages, &mut queue);
                    driver.release_response(reply, messages);
                    let reply = response(&mut driver, pipelined, &mut io, messages, &mut queue);
                    reply
                        .with(messages, |reply| {
                            let owned::Reply::Results(payload) = reply else {
                                panic!("pipelined capability result")
                            };
                            assert_eq!(
                                harness::Owned::ECHO
                                    .read_results(payload.content())
                                    .unwrap()
                                    .value(),
                                123
                            );
                        })
                        .unwrap();
                    driver.release_response(reply, messages);
                    maintain(&mut driver, &mut io, messages, &mut queue);
                    driver
                        .enqueue_call(
                            &target,
                            harness::Owned::BOUNCE,
                            &[&child],
                            messages,
                            BYTES,
                            Default::default(),
                            |mut p| p.field(BounceParams::CAP).set_index(0),
                        )
                        .unwrap();
                    let bounced = driver
                        .send_invocation(
                            messages,
                            &mut queue,
                            BYTES,
                            Default::default(),
                            &mut [0; 4],
                        )
                        .unwrap()
                        .unwrap()
                        .question;
                    let reflected = driver.pipeline(bounced, pipeline::Path::new(&[0])).unwrap();
                    let reply = response(&mut driver, bounced, &mut io, messages, &mut queue);
                    driver.release_response(reply, messages);
                    let echoed = call(&mut driver, &reflected, 29, &mut io, messages, &mut queue);
                    let reply = response(&mut driver, echoed, &mut io, messages, &mut queue);
                    reply
                        .with(messages, |reply| {
                            let owned::Reply::Results(payload) = reply else {
                                panic!("reflected capability result")
                            };
                            assert_eq!(
                                harness::Owned::ECHO
                                    .read_results(payload.content())
                                    .unwrap()
                                    .value(),
                                129
                            );
                        })
                        .unwrap();
                    driver.release_response(reply, messages);
                    let canceled = call(&mut driver, &target, 31, &mut io, messages, &mut queue);
                    driver.cancel(canceled).unwrap();
                    let following = call(&mut driver, &target, 37, &mut io, messages, &mut queue);
                    let reply = response(&mut driver, following, &mut io, messages, &mut queue);
                    reply
                        .with(messages, |reply| {
                            let owned::Reply::Results(payload) = reply else {
                                panic!("call after cancellation")
                            };
                            assert_eq!(
                                harness::Owned::ECHO
                                    .read_results(payload.content())
                                    .unwrap()
                                    .value(),
                                137
                            );
                        })
                        .unwrap();
                    driver.release_response(reply, messages);
                    driver.runtime.release(reflected);
                    driver.runtime.release(child);
                    driver.runtime.release(target);
                    maintain(&mut driver, &mut io, messages, &mut queue);
                    assert!(
                        driver.ledgers.questions.is_empty(),
                        "all Return/Finish obligations completed"
                    );
                    let mut closing = driver.terminate(&mut queue);
                    loop {
                        match closing.poll(messages, budget()).unwrap() {
                            Termination::Done => break,
                            Termination::Pending | Termination::Import(()) => (),
                            _ => panic!("client-only terminal owner"),
                        }
                    }
                    let writers = core::array::from_fn::<_, 12, _>(|_| {
                        messages.reserve(BYTES).expect("all buffers reclaimed")
                    });
                    for writer in writers {
                        messages.discard(writer);
                    }
                });
            },
        )
        .unwrap();
    });
    assert_eq!(
        allocations.count_total, 0,
        "bounded C++ conversation allocated"
    );
    println!("Bounded C++ TCP interop: bootstrap and result pipelines, reflected capabilities, generated calls, exception, cancellation, Finish/Release and complete buffer recovery passed");
}
