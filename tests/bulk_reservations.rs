use capntproto::bulk::{CreditWindow, Reservation, Settlement};

fn metrics(window: &CreditWindow) -> (u32, u32, u64, usize) {
    (
        window.available(),
        window.in_flight(),
        window.issued(),
        window.pending_chunks(),
    )
}

#[test]
fn reservations_follow_moves_and_reject_other_or_replacement_windows() {
    let mut a = CreditWindow::new(3, 2).unwrap();
    let mut b = CreditWindow::new(3, 2).unwrap();
    let ra = a.reserve(1).unwrap().unwrap();
    let rb = b.reserve(2).unwrap().unwrap();
    assert_eq!(ra.sequence(), rb.sequence());
    let before = (metrics(&a), metrics(&b));
    assert!(a.settle(&rb).is_err());
    assert!(b.settle(&ra).is_err());
    assert_eq!((metrics(&a), metrics(&b)), before);
    let mut moved = Box::new(a);
    assert_eq!(moved.settle(&ra).unwrap(), Settlement::Released);
    assert_eq!(moved.settle(&ra).unwrap(), Settlement::AlreadySettled);
    drop(moved);
    // The old reservation keeps the window identity allocation alive. A fresh
    // window at the same numerical sequence cannot accept it, even after move.
    let mut replacement = CreditWindow::new(3, 2).unwrap();
    let fresh = replacement.reserve(3).unwrap().unwrap();
    assert_eq!(fresh.sequence(), ra.sequence());
    let before = metrics(&replacement);
    assert!(replacement.settle(&ra).is_err());
    assert_eq!(metrics(&replacement), before);
    assert_eq!(replacement.settle(&fresh).unwrap(), Settlement::Released);
    assert_eq!(b.settle(&rb).unwrap(), Settlement::Released);
}

#[test]
fn dropped_handles_and_rejected_reservations_do_not_return_credit() {
    let mut w = CreditWindow::new(3, 2).unwrap();
    let first = w.reserve(2).unwrap().unwrap();
    drop(first);
    let before = metrics(&w);
    assert!(w.reserve(2).unwrap().is_none());
    for invalid in [0, 4, u32::MAX] {
        assert!(w.reserve(invalid).is_err());
    }
    assert_eq!(metrics(&w), before);
    let last = w.reserve(1).unwrap().unwrap();
    assert_eq!(last.sequence(), 2);
    assert_eq!(w.settle(&last).unwrap(), Settlement::Released);
    let before = metrics(&w);
    assert!(w.reserve(1).is_err());
    assert_eq!(w.settle(&last).unwrap(), Settlement::AlreadySettled);
    assert_eq!(metrics(&w), before);
    assert_eq!(before, (1, 2, 2, 1));
}

#[test]
fn full_chunk_namespace_is_usable_once_and_remains_exhausted() {
    let mut w = CreditWindow::new(1, 65536).unwrap();
    for sequence in 1..=65536 {
        let reservation = w.reserve(1).unwrap().unwrap();
        assert_eq!(reservation.sequence(), sequence);
        assert_eq!(w.settle(&reservation).unwrap(), Settlement::Released);
    }
    let before = metrics(&w);
    for _ in 0..2 {
        assert!(w.reserve(1).is_err());
    }
    assert_eq!(before, (1, 0, 65536, 0));
    assert_eq!(metrics(&w), before);
}

#[test]
fn replay_tlc_bulk_reservation_ownership() {
    use capntproto_test_support::verification::exploration;
    const MODEL: &str = "verification/BulkReservation.tla";
    const CONFIG: &str = include_str!("../verification/BulkReservation.cfg");
    let paths = exploration::traces(MODEL, "bulk-reservation", CONFIG).unwrap();
    for path in &paths {
        let mut windows = [
            CreditWindow::new(2, 2).unwrap(),
            CreditWindow::new(2, 2).unwrap(),
        ];
        let mut handles: [Option<Reservation>; 2] = [None, None];
        // Independent accounting for actual issued reservations, including ones
        // whose handles were dropped. No model state drives this ledger.
        let mut pending = [[0u64; 2]; 2];
        for state in path {
            let prior = [windows[0].available(), windows[1].available()];
            let event = state["event"];
            let result = match event {
                1..=4 => {
                    let owner = ((event - 1) / 2) as usize;
                    let bytes = (1 + (event - 1) % 2) as u32;
                    assert!(handles[owner].is_none());
                    match windows[owner].reserve(bytes) {
                        Ok(Some(reservation)) => {
                            assert_eq!(reservation.bytes(), bytes);
                            let slot = reservation.sequence() as usize - 1;
                            assert!(slot < 2);
                            assert_eq!(pending[owner][slot], 0);
                            pending[owner][slot] = u64::from(bytes);
                            handles[owner] = Some(reservation);
                            1
                        }
                        Ok(None) => 0,
                        Err(error) => {
                            assert_eq!(error.kind, capnp::ErrorKind::Failed);
                            5
                        }
                    }
                }
                5..=8 => {
                    let owner = ((event - 5) % 2) as usize;
                    let target = if event <= 6 { owner } else { 1 - owner };
                    let reservation = handles[owner].as_ref().unwrap();
                    let before = metrics(&windows[target]);
                    match windows[target].settle(reservation) {
                        Ok(Settlement::Released) => {
                            assert_eq!(target, owner);
                            let debt = &mut pending[owner][reservation.sequence() as usize - 1];
                            assert_eq!(*debt, u64::from(reservation.bytes()));
                            *debt = 0;
                            2
                        }
                        Ok(Settlement::AlreadySettled) => {
                            assert_eq!(target, owner);
                            assert_eq!(pending[owner][reservation.sequence() as usize - 1], 0);
                            assert_eq!(metrics(&windows[target]), before);
                            3
                        }
                        Err(error) => {
                            assert_ne!(owner, target);
                            assert_eq!(error.kind, capnp::ErrorKind::Failed);
                            assert_eq!(metrics(&windows[target]), before);
                            4
                        }
                    }
                }
                9..=10 => {
                    let owner = (event - 9) as usize;
                    let before = metrics(&windows[owner]);
                    drop(handles[owner].take().unwrap());
                    assert_eq!(metrics(&windows[owner]), before);
                    6
                }
                _ => panic!("invalid event: {state:?}"),
            };
            for (field, actual) in [
                ("result", result),
                ("priorA", prior[0].into()),
                ("priorB", prior[1].into()),
                ("ac", windows[0].available().into()),
                ("bc", windows[1].available().into()),
                ("an", windows[0].issued() + 1),
                ("bn", windows[1].issued() + 1),
                ("a1", pending[0][0]),
                ("a2", pending[0][1]),
                ("b1", pending[1][0]),
                ("b2", pending[1][1]),
                ("ah", handles[0].as_ref().map_or(0, Reservation::sequence)),
                ("bh", handles[1].as_ref().map_or(0, Reservation::sequence)),
                ("az", handles[0].as_ref().map_or(0, |r| r.bytes().into())),
                ("bz", handles[1].as_ref().map_or(0, |r| r.bytes().into())),
            ] {
                assert_eq!(actual, state[field], "{field}: {path:?}");
            }
            for owner in 0..2 {
                assert_eq!(
                    windows[owner].in_flight() as u64,
                    pending[owner].iter().sum::<u64>()
                );
                assert_eq!(
                    windows[owner].pending_chunks(),
                    pending[owner].iter().filter(|v| **v != 0).count()
                );
            }
        }
    }
    exploration::controls(
        MODEL,
        "bulk-reservation",
        CONFIG,
        &[
            ("foreign", "ForeignIsInert"),
            ("duplicate", "Conservation"),
            ("drop", "DropIsInert"),
            ("exhaust", "Namespace"),
            ("backpressure", "Conservation"),
        ],
        None,
    )
    .unwrap();
    eprintln!(
        "{} production bulk reservation edge-prefix replays",
        paths.len()
    );
}
