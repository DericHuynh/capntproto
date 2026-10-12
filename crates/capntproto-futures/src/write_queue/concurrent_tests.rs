    #[test]
    fn stale_hint_resamples_outside_locks_after_a_concurrent_batch_start() {
        type Message = Builder<HeapAllocator>;
        let shared = Arc::new(Mutex::new(None::<std::sync::Weak<Shared<Message>>>));
        let observed = shared.clone();
        let (sender, _driver) = write_queue_with_clock(futures::io::sink(), move || {
            let shared = observed
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .upgrade()
                .unwrap();
            assert!(
                shared.queue.try_lock().is_ok(),
                "clock ran under queue lock"
            );
            assert!(
                shared.diagnostics.metrics.try_lock().is_ok(),
                "clock ran under metrics lock"
            );
            Duration::from_secs(17)
        });
        *shared.lock().unwrap() = Some(Arc::downgrade(&sender.shared));
        // Model a producer observing the old hint just as the receiver drains
        // the queue. The locked state, not the hint, determines the timestamp.
        sender.shared.queued.store(true, Ordering::Relaxed);
        sender.send_detached(message());
        assert_eq!(sender.len(), 1);
        assert_eq!(
            sender
                .shared
                .diagnostics
                .metrics
                .lock()
                .unwrap()
                .queued
                .oldest,
            Some(Duration::from_secs(17))
        );
    }

    #[test]
    fn reentrant_clock_can_enqueue_without_changing_the_oldest_timestamp() {
        type Message = Builder<HeapAllocator>;
        let access = Arc::new(Mutex::new(None::<Sender<Message>>));
        let weak_access = Arc::downgrade(&access);
        let entered = AtomicBool::new(false);
        let (sender, driver) = write_queue_with_clock(futures::io::sink(), move || {
            if !entered.swap(true, Ordering::Relaxed) {
                let access = weak_access.upgrade().unwrap();
                let sender = access.lock().unwrap().as_ref().unwrap().clone();
                sender.send_detached(message());
            }
            Duration::from_secs(17)
        });
        *access.lock().unwrap() = Some(sender.clone());
        sender.send_detached(message());
        assert_eq!(sender.len(), 2);
        assert_eq!(sender.outgoing_queue().snapshot().wait_time, Duration::ZERO);
        futures::pin_mut!(driver);
        assert!(driver.as_mut().now_or_never().is_none());
    }
