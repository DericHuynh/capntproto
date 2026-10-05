use super::*;
use capnp::Word;

fn scratch() -> Vec<Word> {
    let mut words = Word::allocate_zeroed_vec(1024);
    Word::words_to_bytes_mut(&mut words).fill(0xa5);
    words
}

#[cfg(target_os = "linux")]
#[tokio::test(flavor = "current_thread")]
async fn fd_scratch_reads_a_buffered_single_segment_without_allocating() {
    let (sender, receiver) = UnixStream::pair().unwrap();
    write_message(&sender, &control(1), &[]).await.unwrap();
    write_message(&sender, &control(2), &[descriptor(22)])
        .await
        .unwrap();
    let mut reader = FdReader::new(receiver, Options::default());
    drop(reader.read().await.unwrap().unwrap());
    assert_eq!(reader.reader.get_ref().ancillary.borrow().fds.len(), 1);
    let mut words = scratch();
    let mut slots = [None];
    let counts = allocation_counter::measure(|| {
        let message = reader
            .try_read_message_with_scratch(&mut words, &mut slots, |_| Ok(false))
            .now_or_never()
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(
            message.body.get_segments().uses_scratch(),
            cfg!(target_os = "linux")
        );
        assert_eq!(message.fds.len(), 1);
        assert_eq!(fd_value(message.fds[0].as_ref().unwrap()), 22);
    });
    assert_eq!(counts.count_total, 0, "{counts:?}");
    assert_eq!(counts.bytes_total, 0);
    assert_eq!(counts.count_current, 0);
    assert!(slots[0].is_some());
}

#[tokio::test(flavor = "current_thread")]
async fn caller_slots_bound_receipts_and_keep_exactly_one_owner() {
    for capacity in [0usize, 1, 2, 4] {
        for configured in [0, 1, 253] {
            let (sender, receiver) = UnixStream::pair().unwrap();
            let (a, mut aw) = witnessed_fd();
            let (b, mut bw) = witnessed_fd();
            let (c, mut cw) = witnessed_fd();
            write_message(&sender, &control(7), &[a, b, c])
                .await
                .unwrap();
            let mut reader = FdReader::new(
                &receiver,
                Options {
                    max_fds: configured,
                    ..Default::default()
                },
            );
            let mut words = scratch();
            let mut slots: Vec<_> = (0..capacity).map(|_| None).collect();
            let count = capacity.min(configured).min(3);
            let message = reader
                .try_read_message_with_scratch(&mut words, &mut slots, |_| Ok(false))
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                message.body.get_segments().uses_scratch(),
                cfg!(target_os = "linux")
            );
            assert_eq!(message.fds.len(), count);
            for fd in message.fds.iter().flatten() {
                assert_ne!(
                    // SAFETY: fd remains owned and live for this flags-only query.
                    unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFD) } & libc::FD_CLOEXEC,
                    0
                );
            }
            drop(message);
            drop(reader);
            for (i, witness) in [&mut aw, &mut bw, &mut cw].into_iter().enumerate() {
                if i < count {
                    open(witness);
                } else {
                    closed(witness);
                }
            }
            // Taking one slot transfers ownership without touching the others.
            let transferred = slots.first_mut().and_then(Option::take);
            slots.clear();
            if transferred.is_some() {
                open(&mut aw);
            }
            drop(transferred);
            for witness in [&mut aw, &mut bw, &mut cw] {
                closed(witness);
            }
        }
    }
}

#[cfg(target_os = "linux")]
#[tokio::test(flavor = "current_thread")]
async fn prefetched_descriptors_survive_rejected_reads_and_smaller_next_slots() {
    let (sender, receiver) = UnixStream::pair().unwrap();
    let (a, mut aw) = witnessed_fd();
    let (b, mut bw) = witnessed_fd();
    write_message(&sender, &control(1), &[]).await.unwrap();
    write_message(&sender, &control(2), &[a, b]).await.unwrap();
    let mut reader = FdReader::new(&receiver, Options::default());
    let mut words = scratch();
    let mut slots = [None, None];
    let first = reader
        .try_read_message_with_scratch(&mut words, &mut slots, |_| Ok(true))
        .await
        .unwrap()
        .unwrap();
    assert!(first.fds.is_empty());
    assert_eq!(reader.reader.get_ref().ancillary.borrow().fds.len(), 2);
    assert!(reader
        .try_read_message_with_scratch(&mut [], &mut [], |_| Ok(false))
        .await
        .is_err());
    assert_eq!(reader.reader.get_ref().ancillary.borrow().fds.len(), 2);
    drop(first);
    let mut occupied = [Some(descriptor(99).as_ref().try_clone().unwrap())];
    assert!(reader
        .try_read_message_with_scratch(&mut words, &mut occupied, |_| Ok(false))
        .await
        .is_err());
    assert_eq!(fd_value(occupied[0].as_ref().unwrap()), 99);
    assert_eq!(reader.reader.get_ref().ancillary.borrow().fds.len(), 2);
    let second = reader
        .try_read_message_with_scratch(&mut words, &mut slots[..1], |_| Ok(false))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(second.fds.len(), 1);
    open(&mut aw);
    closed(&mut bw);
    drop(second);
    assert_eq!(reader.reader.get_ref().ancillary.borrow().reads, 1);
    slots[0].take();
    closed(&mut aw);
}

#[tokio::test(flavor = "current_thread")]
async fn canceled_partial_fd_reads_resume_or_close_on_reader_drop() {
    for size in [64, 3000] {
        let frame = capnp::serialize::write_message_to_words(&*body(size));
        for prefix in [1, 5, 8, 15, frame.len() - 1] {
            for resume in [false, true] {
                let (sender, receiver) = UnixStream::pair().unwrap();
                let (a, mut aw) = witnessed_fd();
                let (b, mut bw) = witnessed_fd();
                write_message(&sender, &frame[..prefix], &[a, b])
                    .await
                    .unwrap();
                let mut reader = FdReader::new(&receiver, Options::default());
                let mut words = scratch();
                let mut slots = [None, None];
                assert!(tokio::time::timeout(
                    std::time::Duration::from_millis(10),
                    reader.try_read_message_with_scratch(&mut words, &mut slots, |_| Ok(false))
                )
                .await
                .is_err());
                assert_eq!(reader.reader.get_ref().ancillary.borrow().fds.len(), 2);
                assert!(slots.iter().all(Option::is_none));
                assert!(Word::words_to_bytes(&words).iter().all(|&v| v == 0xa5));
                open(&mut aw);
                open(&mut bw);
                if resume {
                    write_message(&sender, &frame[prefix..], &[]).await.unwrap();
                    write_message(&sender, &control(2), &[descriptor(22)])
                        .await
                        .unwrap();
                    let message = reader
                        .try_read_message_with_scratch(&mut words, &mut slots[..1], |_| Ok(false))
                        .await
                        .unwrap()
                        .unwrap();
                    assert!(
                        !message.body.get_segments().uses_scratch(),
                        "FD partial frames take the owned direct-read path"
                    );
                    assert_eq!(
                        message.body.get_root::<capnp::data::Reader>().unwrap(),
                        vec![19; size as usize]
                    );
                    assert_eq!(message.fds.len(), 1);
                    open(&mut aw);
                    closed(&mut bw);
                    drop(message);
                    slots[0].take();
                    let next = reader
                        .try_read_message_with_scratch(&mut words, &mut slots, |_| Ok(false))
                        .await
                        .unwrap()
                        .unwrap();
                    assert_eq!(next.fds.len(), 1);
                    assert_eq!(fd_value(next.fds[0].as_ref().unwrap()), 22);
                    drop(next);
                    slots[0].take();
                }
                drop(reader);
                closed(&mut aw);
                closed(&mut bw);
            }
        }
    }
}

#[tokio::test(flavor = "current_thread")]
async fn fd_read_errors_close_staging_and_leave_caller_storage_untouched() {
    for kind in ["eof", "invalid", "limit"]
        .into_iter()
        .chain(cfg!(target_os = "linux").then_some("classifier"))
    {
        let (sender, receiver) = UnixStream::pair().unwrap();
        let (fd, mut witness) = witnessed_fd();
        let bytes = match kind {
            "eof" => control(1)[..7].to_vec(),
            "invalid" => vec![255; 8],
            _ => control(1),
        };
        write_message(&sender, &bytes, &[fd]).await.unwrap();
        drop(sender);
        let mut options = Options::default();
        if kind == "limit" {
            options.max_message_words = 0;
        }
        let mut reader = FdReader::new(&receiver, options);
        let mut words = scratch();
        let mut slots = [None];
        assert!(reader
            .try_read_message_with_scratch(&mut words, &mut slots, |_| {
                Err(Error::failed("classifier failure".into()))
            })
            .await
            .is_err());
        closed(&mut witness);
        assert!(slots[0].is_none());
        assert!(Word::words_to_bytes(&words).iter().all(|&v| v == 0xa5));
        assert!(reader
            .try_read_message_with_scratch(&mut words, &mut slots, |_| Ok(false))
            .await
            .is_err());
    }
}

#[cfg(target_os = "linux")]
#[tokio::test(flavor = "current_thread")]
async fn caller_storage_and_fd_boundaries_match_pinned_cpp() {
    use capntproto_test_support::verification::{command, cpp, root, run};
    let build = cpp::build(&["capnp-rpc"]).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let executable = directory.path().join("buffered-fds-scratch");
    let logs = root().join("target/verification/buffered-fds-scratch");
    run(
        command("g++")
            .args([
                "-std=c++23",
                "-Ivendor/capnproto/c++/src",
                "tests/cpp/buffered-fds.c++",
            ])
            .arg(build.join("c++/src/capnp/libcapnp-rpc.a"))
            .arg(build.join("c++/src/capnp/libcapnp.a"))
            .arg(build.join("c++/src/kj/libkj-async.a"))
            .arg(build.join("c++/src/kj/libkj.a"))
            .args(["-pthread", "-o"])
            .arg(&executable),
        &logs.join("compile.log"),
        0,
    )
    .unwrap();
    let mut observations = 0;
    for extra_words in [0usize, 400] {
        let frames: Vec<_> = (1..=3)
            .map(|id| {
                let mut bytes = control(id);
                assert_eq!(&bytes[..4], &[0; 4]);
                let words = u32::from_le_bytes(bytes[4..8].try_into().unwrap());
                bytes[4..8].copy_from_slice(&(words + extra_words as u32).to_le_bytes());
                bytes.extend(vec![0; extra_words * 8]);
                bytes
            })
            .collect();
        let needed = frames[0].len() / 8;
        let file = directory.path().join(format!("frames-{extra_words}.bin"));
        std::fs::write(&file, frames.concat()).unwrap();
        for capacity in [0, needed - 1, needed, needed + 8] {
            for short in [false, true] {
                let name = format!("{extra_words}-{capacity}-{short}");
                let reference = run(
                    command(&executable)
                        .arg(&file)
                        .arg(capacity.to_string())
                        .arg(u8::from(short).to_string())
                        .arg("256"),
                    &logs.join(format!("{name}-cpp.log")),
                    0,
                )
                .unwrap();
                let mut observed = String::new();
                for limit in [0, 1, 2] {
                    for prefix in [0, 1, 8, frames[1].len() - 1] {
                        for split in [false, true] {
                            if prefix == 0 && split {
                                continue;
                            }
                            let (sender, receiver) = UnixStream::pair().unwrap();
                            write_message(&sender, &frames[0], &[]).await.unwrap();
                            let count = if prefix == 0 { frames[1].len() } else { prefix };
                            let fds = if split {
                                vec![descriptor(11)]
                            } else {
                                vec![descriptor(11), descriptor(12)]
                            };
                            write_message(&sender, &frames[1][..count], &fds)
                                .await
                                .unwrap();
                            if prefix != 0 {
                                let fds = if split { vec![descriptor(12)] } else { vec![] };
                                write_message(&sender, &frames[1][count..], &fds)
                                    .await
                                    .unwrap();
                            }
                            write_message(&sender, &frames[2], &[descriptor(22)])
                                .await
                                .unwrap();
                            drop(sender);
                            let mut reader =
                                FdReader::with_buffer_size(&receiver, Options::default(), 256)
                                    .unwrap();
                            let mut slots: Vec<_> = (0..limit).map(|_| None).collect();
                            let mut words = Word::allocate_zeroed_vec(capacity);
                            for expected in 1..=3 {
                                Word::words_to_bytes_mut(&mut words).fill(0xa5);
                                let message = reader
                                    .try_read_message_with_scratch(&mut words, &mut slots, |_| {
                                        Ok(short)
                                    })
                                    .await
                                    .unwrap()
                                    .unwrap();
                                let capnp_rpc::rpc_capnp::message::Finish(finish) = message
                                    .body
                                    .get_root::<capnp_rpc::rpc_capnp::message::Reader>()
                                    .unwrap()
                                    .which()
                                    .unwrap()
                                else {
                                    panic!("expected Finish")
                                };
                                let actual = finish.unwrap().get_question_id();
                                assert_eq!(actual, expected);
                                observed += &format!(
                                    "{limit} {prefix} {} {expected} {} {}",
                                    u8::from(split),
                                    u8::from(message.body.get_segments().is_shared_buffer()),
                                    message.fds.len()
                                );
                                for fd in message.fds.iter_mut() {
                                    observed += &format!(" {}", fd_value(fd.as_ref().unwrap()));
                                    fd.take();
                                }
                                let borrowed = message.body.get_segments().uses_scratch();
                                drop(message);
                                let used = if borrowed { frames[0].len() } else { 0 };
                                let tail = Word::words_to_bytes(&words)[used..]
                                    .iter()
                                    .all(|&v| v == 0xa5);
                                observed +=
                                    &format!(" {} {}\n", u8::from(borrowed), u8::from(tail));
                                observations += 1;
                            }
                            assert!(reader
                                .try_read_message_with_scratch(&mut words, &mut slots, |_| Ok(
                                    false
                                ))
                                .await
                                .unwrap()
                                .is_none());
                        }
                    }
                }
                std::fs::create_dir_all(&logs).unwrap();
                std::fs::write(logs.join(format!("{name}-rust.log")), &observed).unwrap();
                assert_eq!(observed, reference, "scratch case {name}");
            }
        }
    }
    assert_eq!(observations, 1008);
}
