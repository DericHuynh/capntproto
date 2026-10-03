use crossbeam_queue::ArrayQueue;
use serde_json::{json, Value};
use std::{
    collections::VecDeque,
    sync::{Arc, Barrier, Mutex},
    thread,
    time::Instant,
};
use tokio::sync::mpsc;

const CAPACITY: usize = 64;
const TOTAL: usize = 65_536;

struct Message {
    producer: usize,
    sequence: usize,
    started: Option<Instant>,
}

#[derive(Clone)]
enum Sender {
    Standard(Arc<Mutex<VecDeque<Message>>>),
    Parking(Arc<parking_lot::Mutex<VecDeque<Message>>>),
    Array(Arc<ArrayQueue<Message>>),
    Tokio(mpsc::Sender<Message>),
}

enum Receiver {
    Shared(Sender),
    Tokio(mpsc::Receiver<Message>),
}

fn push(q: &mut VecDeque<Message>, message: Message) -> Result<(), Message> {
    if q.len() == CAPACITY {
        Err(message)
    } else {
        q.push_back(message);
        Ok(())
    }
}

impl Sender {
    fn send(&self, message: Message) -> Result<(), Message> {
        match self {
            Self::Standard(q) => push(&mut q.lock().unwrap(), message),
            Self::Parking(q) => push(&mut q.lock(), message),
            Self::Array(q) => q.push(message),
            Self::Tokio(tx) => tx.try_send(message).map_err(|e| match e {
                mpsc::error::TrySendError::Full(message) => message,
                mpsc::error::TrySendError::Closed(_) => panic!("receiver closed early"),
            }),
        }
    }
}

impl Receiver {
    fn drain(&mut self, batch: usize, output: &mut Vec<Message>) {
        match self {
            Self::Shared(Sender::Standard(q)) => {
                let mut q = q.lock().unwrap();
                let count = batch.min(q.len());
                output.extend(q.drain(..count));
            }
            Self::Shared(Sender::Parking(q)) => {
                let mut q = q.lock();
                let count = batch.min(q.len());
                output.extend(q.drain(..count));
            }
            Self::Shared(Sender::Array(q)) => {
                for _ in 0..batch {
                    match q.pop() {
                        Some(message) => output.push(message),
                        None => break,
                    }
                }
            }
            Self::Tokio(rx) => {
                for _ in 0..batch {
                    match rx.try_recv() {
                        Ok(message) => output.push(message),
                        Err(mpsc::error::TryRecvError::Empty) => break,
                        Err(mpsc::error::TryRecvError::Disconnected) => {
                            panic!("senders closed early")
                        }
                    }
                }
            }
            Self::Shared(Sender::Tokio(_)) => unreachable!(),
        }
    }
}

pub fn run(variant: &str, producers: usize, batch: usize) -> Value {
    assert!([1, 16].contains(&batch));
    let (sender, mut receiver) = match variant {
        "tokio" => {
            let (tx, rx) = mpsc::channel(CAPACITY);
            (Sender::Tokio(tx), Receiver::Tokio(rx))
        }
        name => {
            let sender = match name {
                "std" => Sender::Standard(Arc::new(Mutex::new(VecDeque::with_capacity(CAPACITY)))),
                "parking" => Sender::Parking(Arc::new(parking_lot::Mutex::new(
                    VecDeque::with_capacity(CAPACITY),
                ))),
                "array" => Sender::Array(Arc::new(ArrayQueue::new(CAPACITY))),
                _ => panic!("unknown queue {name}"),
            };
            (sender.clone(), Receiver::Shared(sender))
        }
    };
    let barrier = Arc::new(Barrier::new(producers + 2));
    let mut handles = Vec::new();
    for producer in 0..producers {
        let sender = sender.clone();
        let barrier = barrier.clone();
        handles.push(thread::spawn(move || {
            barrier.wait();
            let mut retries = 0u64;
            for sequence in 0..TOTAL / producers {
                let mut message = Message {
                    producer,
                    sequence,
                    started: sequence.is_multiple_of(64).then(Instant::now),
                };
                while let Err(returned) = sender.send(message) {
                    message = returned;
                    retries += 1;
                    thread::yield_now();
                }
            }
            retries
        }));
    }
    let consumer_barrier = barrier.clone();
    let consumer = thread::spawn(move || {
        let mut expected = vec![0; producers];
        let mut messages = Vec::with_capacity(batch);
        let mut samples = Vec::with_capacity(TOTAL / 64);
        let mut empty = 0u64;
        let mut count = 0;
        consumer_barrier.wait();
        while count != TOTAL {
            receiver.drain(batch, &mut messages);
            if messages.is_empty() {
                empty += 1;
                thread::yield_now();
            }
            for message in messages.drain(..) {
                assert_eq!(message.sequence, expected[message.producer]);
                expected[message.producer] += 1;
                count += 1;
                if let Some(started) = message.started {
                    samples.push(started.elapsed().as_nanos() as u64);
                }
            }
        }
        assert!(expected.into_iter().all(|n| n == TOTAL / producers));
        (samples, empty)
    });
    let started = Instant::now();
    barrier.wait();
    let full_retries: u64 = handles.into_iter().map(|h| h.join().unwrap()).sum();
    let (samples, empty_polls) = consumer.join().unwrap();
    let mut result = super::metrics(TOTAL, started.elapsed(), samples);
    result["capacity"] = json!(CAPACITY);
    result["full_retries"] = json!(full_retries);
    result["empty_polls"] = json!(empty_polls);
    result
}
