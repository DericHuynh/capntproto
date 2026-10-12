//! Explicit application state and callback protocol for WebSockets and gRPC.
//! Ephemeral cells are consumed by read; retained cells are released after the
//! trial. Leases and a hard limit bound state even when a client disappears.
use crate::grpc::{operation::Kind, Operation};
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};
use tonic::Status;

#[derive(Default)]
pub(super) struct State {
    next: u64,
    cells: HashMap<u64, Cell>,
}
struct Cell {
    version: u64,
    payload: Vec<u8>,
    ephemeral: bool,
    touched: Instant,
}
pub(super) fn request(kind: Kind, sequence: u64, payload: &[u8]) -> Operation {
    Operation {
        kind: kind as i32,
        sequence,
        payload: payload.to_vec(),
        ..Default::default()
    }
}
pub(super) fn expect(value: &Operation, kind: Kind, sequence: u64) -> crate::Result<()> {
    if value.kind != kind as i32 || value.sequence != sequence {
        return Err("application response kind/sequence mismatch".into());
    }
    Ok(())
}
pub(super) fn notify(mut value: Operation) -> Result<Operation, Status> {
    if value.kind != Kind::Callback as i32 {
        return Err(Status::invalid_argument("expected callback"));
    }
    value.kind = Kind::Notify as i32;
    Ok(value)
}
pub(super) fn finish(ack: Operation, sequence: u64) -> Result<Operation, Status> {
    if ack.kind != Kind::Ack as i32 || ack.sequence != sequence || !ack.payload.is_empty() {
        return Err(Status::invalid_argument(
            "incorrect callback acknowledgement",
        ));
    }
    Ok(request(Kind::Done, sequence, &[]))
}
impl State {
    pub(super) fn apply(&mut self, mut req: Operation) -> Result<Operation, Status> {
        let kind =
            Kind::try_from(req.kind).map_err(|_| Status::invalid_argument("unknown operation"))?;
        match kind {
            Kind::Echo => {}
            Kind::Open => {
                let now = Instant::now();
                self.cells
                    .retain(|_, c| now.duration_since(c.touched) < Duration::from_secs(60));
                if self.cells.len() >= 1024 {
                    return Err(Status::resource_exhausted("cell limit"));
                }
                self.next = self
                    .next
                    .checked_add(1)
                    .ok_or_else(|| Status::resource_exhausted("cell IDs"))?;
                req.cell = self.next;
                self.cells.insert(
                    req.cell,
                    Cell {
                        version: 0,
                        payload: std::mem::take(&mut req.payload),
                        ephemeral: req.ephemeral,
                        touched: now,
                    },
                );
                req.version = 0;
            }
            Kind::Read | Kind::Write | Kind::Release => {
                let cell = self
                    .cells
                    .get_mut(&req.cell)
                    .ok_or_else(|| Status::not_found("unknown cell"))?;
                cell.touched = Instant::now();
                let remove = kind == Kind::Release || (kind == Kind::Read && cell.ephemeral);
                if kind == Kind::Write {
                    if req.version != cell.version {
                        return Err(Status::failed_precondition("version mismatch"));
                    }
                    cell.version = cell
                        .version
                        .checked_add(1)
                        .ok_or_else(|| Status::resource_exhausted("version overflow"))?;
                    cell.payload = std::mem::take(&mut req.payload);
                }
                req.version = cell.version;
                if kind == Kind::Read {
                    req.payload = cell.payload.clone();
                }
                if remove {
                    self.cells.remove(&req.cell);
                }
            }
            _ => return Err(Status::invalid_argument("unexpected operation")),
        }
        Ok(req)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn state_versions_consumption_and_retained_release() {
        let mut state = State::default();
        for ephemeral in [false, true] {
            let mut open = request(Kind::Open, 0, &[1, 2, 3]);
            open.ephemeral = ephemeral;
            let cell = state.apply(open).unwrap().cell;
            let mut read = request(Kind::Read, 1, &[]);
            read.cell = cell;
            assert_eq!(state.apply(read.clone()).unwrap().payload, [1, 2, 3]);
            if ephemeral {
                assert!(state.apply(read).is_err());
                continue;
            }
            let mut write = request(Kind::Write, 2, &[4]);
            write.cell = cell;
            write.version = 1;
            assert!(state.apply(write.clone()).is_err());
            write.version = 0;
            assert_eq!(state.apply(write).unwrap().version, 1);
            let response = state.apply(read.clone()).unwrap();
            assert_eq!(response.version, 1);
            assert_eq!(response.payload, [4]);
            read.kind = Kind::Release as i32;
            state.apply(read.clone()).unwrap();
            assert!(state.apply(read).is_err());
        }
        assert!(state.cells.is_empty());
    }
    #[test]
    fn state_is_bounded_and_expired_cells_are_reclaimed() {
        let mut state = State::default();
        for _ in 0..1024 {
            state.apply(request(Kind::Open, 0, &[])).unwrap();
        }
        assert!(state.apply(request(Kind::Open, 0, &[])).is_err());
        for cell in state.cells.values_mut() {
            cell.touched = Instant::now() - Duration::from_secs(61);
        }
        state.apply(request(Kind::Open, 0, &[])).unwrap();
        assert_eq!(state.cells.len(), 1);
    }
    #[test]
    fn callbacks_require_a_real_matching_acknowledgement() {
        assert!(notify(request(Kind::Echo, 4, &[])).is_err());
        assert_eq!(
            notify(request(Kind::Callback, 4, &[7])).unwrap().payload,
            [7]
        );
        assert!(finish(request(Kind::Done, 4, &[]), 4).is_err());
        assert!(finish(request(Kind::Ack, 3, &[]), 4).is_err());
        assert!(finish(request(Kind::Ack, 4, &[7]), 4).is_err());
        expect(
            &finish(request(Kind::Ack, 4, &[]), 4).unwrap(),
            Kind::Done,
            4,
        )
        .unwrap();
    }
}
