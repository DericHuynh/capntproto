use super::*;
use capnp_rpc::twoparty::ConnectionOptions;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

#[tokio::test(flavor = "current_thread")]
async fn configured_roles_and_graceful_scope_work_for_both_protocols() {
    local(async {
        for _server_protocol in protocol_drivers() {
            for _client_protocol in protocol_drivers() {
                let (a, b) = tokio::io::duplex(43);
                let clock_calls = Arc::new(AtomicUsize::new(0));
                let clock = {
                    let calls = clock_calls.clone();
                    move || {
                        calls.fetch_add(1, Ordering::Relaxed);
                        Duration::ZERO
                    }
                };
                let server = ConnectionOptions::new()
                    .server_with_clock(a.compat(), bootstrap(), clock.clone())
                    .unwrap();
                let server_task = tokio::task::spawn_local(server);
                let drops = Rc::new(Cell::new(0));
                let mut driver = ConnectionOptions::new()
                    .client_with_clock(
                        Tracked {
                            io: b.compat(),
                            drops: drops.clone(),
                        },
                        clock,
                    )
                    .unwrap();
                let remote: harness::Client = driver.bootstrap();
                let disconnected = driver.on_disconnect();
                let output = driver
                    .run(async {
                        assert_eq!(echo(&remote, 42).await?, 42);
                        assert!(echo(&remote, u32::MAX)
                            .await
                            .unwrap_err()
                            .to_string()
                            .contains("intentional failure"));
                        echo(&remote, 73).await
                    })
                    .await
                    .unwrap()
                    .unwrap();
                assert_eq!(output, 73);
                disconnected.await.unwrap();
                assert_eq!(drops.get(), 1, "shutdown observer retained transport");
                assert!(clock_calls.load(Ordering::Relaxed) > 0);
                let _ = server_task.await.unwrap(); // The peer observes the disconnect Abort.
            }
        }
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn scoped_completion_cancels_driver_and_preserves_application_errors() {
    local(async {
        for _protocol in protocol_drivers() {
            let drops = Rc::new(Cell::new(0));
            let (a, b) = tokio::io::duplex(43);
            let server = ConnectionOptions::new()
                .server(a.compat(), bootstrap())
                .unwrap();
            let server_task = tokio::task::spawn_local(server);
            let mut driver = ConnectionOptions::new()
                .client(Tracked {
                    io: b.compat(),
                    drops: drops.clone(),
                })
                .unwrap();
            let remote: harness::Client = driver.bootstrap();
            let disconnected = driver.on_disconnect();
            let output: capnp::Result<()> = driver
                .run_until(async {
                    assert_eq!(echo(&remote, 73).await?, 73);
                    Err(Error::failed("application failed".into()))
                })
                .await
                .unwrap();
            assert!(output
                .unwrap_err()
                .to_string()
                .contains("application failed"));
            assert_eq!(
                disconnected.await.unwrap_err().kind,
                ErrorKind::Disconnected
            );
            assert_eq!(drops.get(), 1);
            let _ = server_task.await.unwrap();
        }
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn application_does_not_hide_early_driver_failure() {
    local(async {
        for _protocol in protocol_drivers() {
            for graceful in [false, true] {
                let driver = ConnectionOptions::new()
                    .client(BrokenIo {
                        closes: Rc::new(Cell::new(0)),
                        fail_read: true,
                        block_close: false,
                    })
                    .unwrap();
                let application = futures::future::pending::<()>();
                let error = if graceful {
                    driver.run(application).await.unwrap_err()
                } else {
                    driver.run_until(application).await.unwrap_err()
                };
                // Legacy may normalize a terminal input failure to clean EOF.
                // Either way an unfinished application must get an error.
                assert!(matches!(
                    error.kind,
                    ErrorKind::Failed | ErrorKind::Disconnected
                ));
            }
        }
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn dropping_application_scope_releases_driver_and_io() {
    local(async {
        for _protocol in protocol_drivers() {
            let drops = Rc::new(Cell::new(0));
            let (a, _peer) = tokio::io::duplex(43);
            let driver = ConnectionOptions::new()
                .client(Tracked {
                    io: a.compat(),
                    drops: drops.clone(),
                })
                .unwrap();
            let observer = driver.on_disconnect();
            let mut scope = Box::pin(driver.run(futures::future::pending::<()>()));
            assert!(futures::poll!(scope.as_mut()).is_pending());
            drop(scope);
            assert_eq!(drops.get(), 1);
            assert_eq!(observer.await.unwrap_err().kind, ErrorKind::Disconnected);
        }
    })
    .await;
}

#[tokio::test(flavor = "current_thread")]
async fn final_driver_poll_can_complete_the_application_without_repolling_a_finished_driver() {
    local(async {
        for _protocol in protocol_drivers() {
            for graceful in [false, true] {
                let driver = ConnectionOptions::new()
                    .client(futures::io::Cursor::new(Vec::<u8>::new()))
                    .unwrap();
                let application = driver.on_disconnect();
                let output = if graceful {
                    driver.run(application).await
                } else {
                    driver.run_until(application).await
                };
                output.unwrap().unwrap();
            }
        }
    })
    .await;
}
