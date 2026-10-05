use super::*;
const TIMEOUT: Duration = Duration::from_secs(15);

#[tokio::test(start_paused = true)]
async fn endpoint_deadlines_survive_blocked_udp_output_without_driver_progress() {
    for length in [1, 1_000_000] {
        let mut fixture = Fixture::configured(Network::default(), false, false, |config| {
            config.set_initial_max_stream_data_bidi_local(65536);
            config.set_initial_max_stream_data_bidi_remote(65536);
        });
        fixture.establish().await;
        fixture.network.sending(fixture.peers[0].id, Send::Blocked);
        let timeout = Duration::from_millis(100);
        let (offer, mut input) = fixture.peers[1].bulk.receive(length, timeout).unwrap();
        let mut output = fixture.peers[0].bulk.send(offer, timeout).unwrap();
        let mut producer = Task::new(async move {
            output.write_all(&vec![1; length as usize]).await?;
            output.finish().await
        });
        let mut consumer = Task::new(async move {
            input.read_to_end(&mut Vec::new()).await?;
            Ok(())
        });
        producer.poll();
        consumer.poll();
        fixture.poll();
        tokio::time::advance(Duration::from_millis(150)).await;
        // Deliberately do not poll either network driver: the public endpoint
        // timers must wake both waits, including a producer awaiting finish.
        producer.poll();
        consumer.poll();
        assert_eq!(producer.result, Some(Err(io::ErrorKind::TimedOut)));
        assert_eq!(consumer.result, Some(Err(io::ErrorKind::TimedOut)));
        fixture.network.sending(fixture.peers[0].id, Send::Ready);
        for _ in 0..20 {
            fixture.tick().await;
        }
        for peer in &fixture.peers {
            assert_eq!(peer.bulk.stats().unwrap().active, 0);
        }
    }
}

#[tokio::test(start_paused = true)]
async fn split_planes_survive_loss_reordering_duplicates_and_crossed_drain() {
    for psk in [false, true] {
        let mut fixture = Fixture::configured(Network::default(), psk, false, |config| {
            config.set_initial_max_stream_data_bidi_local(65536);
            config.set_initial_max_stream_data_bidi_remote(65536);
        });
        fixture.establish().await;
        let (offer, mut input) = fixture.peers[1].bulk.receive(300_000, TIMEOUT).unwrap();
        let mut output = fixture.peers[0].bulk.send(offer, TIMEOUT).unwrap();
        let mut producer = Task::new(async move {
            output.write_all(&vec![71; 300_000]).await?;
            output.finish().await
        });
        let mut consumer = Task::new(async move {
            let mut bytes = Vec::new();
            input.read_to_end(&mut bytes).await?;
            assert_eq!(bytes, vec![71; 300_000]);
            Ok(())
        });
        fixture.peers[0].app.write_all(b"control").await.unwrap();
        // Request drain with admitted bulk still incomplete. Neither side may
        // issue its final connection close before all reliable planes finish.
        for peer in &mut fixture.peers {
            peer.control.begin(Duration::from_secs(15)).unwrap();
            peer.app.shutdown().await.unwrap();
        }
        let mut ordinal = 0;
        let mut delayed = VecDeque::new();
        for turn in 0..6000 {
            producer.poll();
            consumer.poll();
            fixture.poll();
            fixture.read();
            while let Some(packet) = fixture.network.take(turn % 2 == 0) {
                ordinal += 1;
                match ordinal {
                    1 | 5 => (),
                    2 => delayed.push_back((turn + 25, packet)),
                    3 => {
                        fixture.network.deliver(packet.clone());
                        fixture.network.deliver(packet);
                    }
                    _ => fixture.network.deliver(packet),
                }
            }
            while delayed.front().is_some_and(|(until, _)| turn >= *until) {
                fixture.network.deliver(delayed.pop_front().unwrap().1);
            }
            packet_tick().await;
            if fixture.peers.iter().all(|p| p.driver.result.is_some()) && delayed.is_empty() {
                break;
            }
        }
        producer.poll();
        consumer.poll();
        assert!(ordinal >= 5);
        assert_eq!(producer.result, Some(Ok(())));
        assert_eq!(consumer.result, Some(Ok(())));
        assert_eq!(fixture.peers[1].received, b"control");
        for peer in &fixture.peers {
            assert_eq!(peer.driver.result, Some(Ok(())));
            assert!(peer.control.wait().now_or_never().unwrap().is_ok());
        }
    }
}

#[tokio::test(start_paused = true)]
async fn seven_stalled_streams_cannot_starve_eighth_at_minimum_connection_credit() {
    let mut fixture = Fixture::configured(Network::default(), false, false, |config| {
        config.set_initial_max_data(128 * 1024);
        config.set_initial_max_stream_data_bidi_local(65536);
        config.set_initial_max_stream_data_bidi_remote(65536);
    });
    fixture.establish().await;
    let mut readers = Vec::new();
    let mut writers = Vec::new();
    for _ in 0..7 {
        let (offer, input) = fixture.peers[1].bulk.receive(1_000_000, TIMEOUT).unwrap();
        readers.push(input);
        let mut output = fixture.peers[0].bulk.send(offer, TIMEOUT).unwrap();
        writers.push(Task::new(async move {
            output.write_all(&vec![1; 1_000_000]).await?;
            output.finish().await
        }));
    }
    for _ in 0..100 {
        for task in &mut writers {
            task.poll();
        }
        fixture.tick().await;
    }
    let (offer, mut input) = fixture.peers[1].bulk.receive(64_000, TIMEOUT).unwrap();
    let mut output = fixture.peers[0].bulk.send(offer, TIMEOUT).unwrap();
    let mut producer = Task::new(async move {
        output.write_all(&vec![42; 64_000]).await?;
        output.finish().await
    });
    let mut consumer = Task::new(async move {
        let mut bytes = Vec::new();
        input.read_to_end(&mut bytes).await?;
        assert_eq!(bytes, vec![42; 64_000]);
        Ok(())
    });
    fixture.peers[0].app.write_all(b"live").await.unwrap();
    for _ in 0..3000 {
        for task in &mut writers {
            task.poll();
        }
        producer.poll();
        consumer.poll();
        fixture.tick().await;
        fixture.read();
        assert!(fixture.peers[0].bulk.stats().unwrap().outstanding_bytes <= 64 * 1024);
        if producer.result.is_some()
            && consumer.result.is_some()
            && !fixture.peers[1].received.is_empty()
        {
            break;
        }
    }
    assert_eq!(producer.result, Some(Ok(())));
    assert_eq!(consumer.result, Some(Ok(())));
    assert_eq!(fixture.peers[1].received, b"live");
    assert!(writers.iter().all(|task| task.result.is_none()));
}

#[tokio::test(start_paused = true)]
async fn stream_grant_survives_connection_migration_without_rebinding() {
    let mut fixture = Fixture::configured(Network::default(), false, true, |config| {
        config.set_initial_max_stream_data_bidi_local(65536);
        config.set_initial_max_stream_data_bidi_remote(65536);
    });
    fixture.establish().await;
    for _ in 0..30 {
        fixture.tick().await;
    }
    let (offer, mut input) = fixture.peers[1].bulk.receive(100_000, TIMEOUT).unwrap();
    let control = fixture.peers[0].mobility.clone();
    let socket = fixture.network.bind("127.0.0.1:2345".parse().unwrap());
    let mut migration = Box::pin(control.migrate_socket(
        transport::socket::DatagramSocket::Simulated(socket),
        ADDRESSES[1].parse().unwrap(),
        Duration::from_secs(2),
    ));
    let mut migrated = false;
    for _ in 0..1000 {
        if let Some(result) = migration.as_mut().now_or_never() {
            result.unwrap();
            migrated = true;
            break;
        }
        fixture.tick().await;
    }
    assert!(migrated);
    let mut output = fixture.peers[0].bulk.send(offer, TIMEOUT).unwrap();
    let mut producer = Task::new(async move {
        output.write_all(&vec![3; 100_000]).await?;
        output.finish().await
    });
    let mut consumer = Task::new(async move {
        let mut bytes = Vec::new();
        input.read_to_end(&mut bytes).await?;
        assert_eq!(bytes, vec![3; 100_000]);
        Ok(())
    });
    for _ in 0..2000 {
        producer.poll();
        consumer.poll();
        fixture.tick().await;
        if producer.result.is_some() && consumer.result.is_some() {
            break;
        }
    }
    assert_eq!(producer.result, Some(Ok(())));
    assert_eq!(consumer.result, Some(Ok(())));
}
