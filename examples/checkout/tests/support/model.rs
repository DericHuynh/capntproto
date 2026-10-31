//! Independent user-level oracle. This module never calls production transition,
//! quote, SQL, compensation or audit-validation functions.
use capntproto_checkout_example::{Audit, BusinessError, Snapshot, State, Status};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
#[derive(Clone, Debug, Serialize)]
pub enum Operation {
    Place(u32),
    Cancel,
    View,
}
#[derive(Clone, Debug, Serialize)]
pub struct Record {
    pub start: u64,
    pub end: u64,
    pub operation: Operation,
    pub result: Result<Snapshot, BusinessError>,
}
#[derive(Clone, Debug)]
struct Reference {
    quantity: u32,
    status: Status,
}
fn advance(current: Option<&Reference>) -> Vec<Option<Reference>> {
    let mut choices = vec![current.cloned()];
    if let Some(current) = current {
        for status in match current.status {
            Status::Processing => &[Status::Confirmed, Status::Declined][..],
            Status::Cancelling => &[Status::Cancelled][..],
            _ => &[],
        } {
            choices.push(Some(Reference {
                quantity: current.quantity,
                status: *status,
            }));
        }
    }
    choices
}
fn step(current: Option<&Reference>, record: &Record, key: &str) -> Vec<Option<Reference>> {
    let mut next = Vec::new();
    for before in advance(current) {
        let expected = match (&record.operation, &before) {
            (Operation::Place(quantity), _) if !(1..=8).contains(quantity) => {
                Err(BusinessError::Invalid)
            }
            (Operation::Place(quantity), Some(value))
                if value.quantity != 0 && value.quantity != *quantity =>
            {
                Err(BusinessError::Conflict)
            }
            (Operation::Place(quantity), None) => Ok(Reference {
                quantity: *quantity,
                status: Status::Processing,
            }),
            (Operation::View, None) => Err(BusinessError::NotFound),
            (Operation::Cancel, None) => Ok(Reference {
                quantity: 0,
                status: Status::Cancelled,
            }),
            (Operation::Cancel, Some(value)) if !value.status.terminal() => Ok(Reference {
                quantity: value.quantity,
                status: Status::Cancelling,
            }),
            (_, Some(value)) => Ok(value.clone()),
        };
        match (&expected, &record.result) {
            (Err(want), Err(got)) if want == got => next.push(before.clone()),
            (Ok(want), Ok(got))
                if got.key == key
                    && got.quantity == want.quantity
                    && got.status == want.status
                    && got.cents
                        == u64::from(want.quantity)
                            * if want.quantity >= 3 { 900 } else { 1000 } =>
            {
                next.push(Some(want.clone()))
            }
            _ => {}
        }
    }
    next
}
/// Exhaust all legal linearizations, respecting real-time (non-overlap) order.
/// Internal confirmation/decline/compensation may occur between public operations.
pub fn linearizable(records: &[Record], key: &str) -> bool {
    fn search(records: &[Record], remaining: u64, current: Option<&Reference>, key: &str) -> bool {
        if remaining == 0 {
            return true;
        }
        for (index, record) in records.iter().enumerate() {
            let bit = 1 << index;
            if remaining & bit == 0 {
                continue;
            }
            if records
                .iter()
                .enumerate()
                .any(|(other, prior)| remaining & (1 << other) != 0 && prior.end < record.start)
            {
                continue;
            }
            for after in step(current, record, key) {
                if search(records, remaining ^ bit, after.as_ref(), key) {
                    return true;
                }
            }
        }
        false
    }
    if records.len() > 16 {
        return false;
    }
    // Observations after a completed operation may never go backwards or alter a terminal receipt.
    for a in records {
        for b in records {
            if a.end < b.start {
                if let (Ok(a), Ok(b)) = (&a.result, &b.result) {
                    if b.revision < a.revision || a.status.terminal() && a != b {
                        return false;
                    }
                }
            }
        }
    }
    search(records, (1 << records.len()) - 1, None, key)
}
/// At quiescence every publicly accepted order must exist, and every resource
/// effect must correspond exactly to one confirmed user receipt.
pub fn audit(
    orders: &BTreeMap<(String, String), Snapshot>,
    accepted: &BTreeSet<(String, String)>,
    inventory: &Audit,
    payments: &Audit,
) -> Result<(), String> {
    for id in accepted {
        if !orders.contains_key(id) {
            return Err(format!("lost accepted order {id:?}"));
        }
    }
    if orders.values().any(|o| !o.status.terminal()) {
        return Err("unfinished user order".into());
    }
    for (resource, is_inventory) in [(inventory, true), (payments, false)] {
        let mut keys = BTreeSet::new();
        for entry in &resource.entries {
            if !keys.insert(&entry.claim.key) {
                return Err("duplicate resource effect".into());
            }
            if entry.state == State::Held {
                return Err("leaked hold after settlement".into());
            }
            let (account, key) = entry.claim.key.split_once('/').ok_or("bad resource id")?;
            let order = orders
                .get(&(account.into(), key.into()))
                .ok_or("orphan resource effect")?;
            let expected_units = if is_inventory {
                u64::from(order.quantity)
            } else {
                order.cents
            };
            let bucket = if is_inventory { "tickets" } else { account };
            let expected_fingerprint = format!("{}:{}:{}", account, order.quantity, order.cents);
            if entry.claim.units != expected_units
                || entry.claim.bucket != bucket
                || entry.claim.fingerprint != expected_fingerprint
            {
                return Err("wrong reservation or charge".into());
            }
            if (entry.state == State::Committed) != (order.status == Status::Confirmed) {
                return Err("user receipt disagrees with charge/stock effect".into());
            }
        }
        for bucket in &resource.buckets {
            let held = resource
                .entries
                .iter()
                .filter(|e| {
                    e.claim.bucket == bucket.name
                        && matches!(e.state, State::Held | State::Committed)
                })
                .try_fold(0u64, |sum, e| sum.checked_add(e.claim.units))
                .ok_or("sum overflow")?;
            if bucket.available.checked_add(held) != Some(bucket.initial) {
                return Err("stock/money conservation violated".into());
            }
        }
        for ((account, key), order) in orders {
            if order.status == Status::Confirmed {
                let id = format!("{account}/{key}");
                if !resource
                    .entries
                    .iter()
                    .any(|e| e.claim.key == id && e.state == State::Committed)
                {
                    return Err("confirmed receipt missing resource effect".into());
                }
            }
        }
    }
    for order in orders.values() {
        let expected = u64::from(order.quantity) * if order.quantity >= 3 { 900 } else { 1000 };
        if order.cents != expected {
            return Err("incorrect user price".into());
        }
    }
    Ok(())
}
