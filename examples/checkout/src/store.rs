//! Each service commits its own state atomically; no transaction spans RPC.
use crate::{checkout_capnp as wire, error, quote, valid_key, Result};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use std::path::Path;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Account {
    pub name: String,
    pub token: String,
    pub cents: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    pub stock: u64,
    pub accounts: Vec<Account>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Status {
    Processing,
    Cancelling,
    Confirmed,
    Cancelled,
    Declined,
}
impl Status {
    pub fn terminal(self) -> bool {
        matches!(self, Self::Confirmed | Self::Cancelled | Self::Declined)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub key: String,
    pub quantity: u32,
    pub cents: u64,
    pub status: Status,
    pub revision: u64,
}
impl Snapshot {
    pub(crate) fn write(&self, mut out: wire::snapshot::Builder<'_>) {
        out.set_key(&self.key);
        out.set_quantity(self.quantity);
        out.set_cents(self.cents);
        out.set_revision(self.revision);
        out.set_status(match self.status {
            Status::Processing => wire::Status::Processing,
            Status::Cancelling => wire::Status::Cancelling,
            Status::Confirmed => wire::Status::Confirmed,
            Status::Cancelled => wire::Status::Cancelled,
            Status::Declined => wire::Status::Declined,
        });
    }
    pub(crate) fn read(input: wire::snapshot::Reader<'_>) -> crate::capnp::Result<Self> {
        Ok(Self {
            key: input.get_key()?.to_str()?.into(),
            quantity: input.get_quantity(),
            cents: input.get_cents(),
            revision: input.get_revision(),
            status: match input.get_status()? {
                wire::Status::Processing => Status::Processing,
                wire::Status::Cancelling => Status::Cancelling,
                wire::Status::Confirmed => Status::Confirmed,
                wire::Status::Cancelled => Status::Cancelled,
                wire::Status::Declined => Status::Declined,
            },
        })
    }
}
#[derive(Clone, Debug)]
pub struct OrderRow {
    pub account: String,
    pub snapshot: Snapshot,
    pub intent: String,
}
impl OrderRow {
    pub fn id(&self) -> String {
        format!("{}/{}", self.account, self.snapshot.key)
    }
    pub fn inventory(&self) -> Claim {
        self.claim("tickets".into(), u64::from(self.snapshot.quantity))
    }
    pub fn payment(&self) -> Claim {
        self.claim(self.account.clone(), self.snapshot.cents)
    }
    fn claim(&self, bucket: String, units: u64) -> Claim {
        Claim {
            key: self.id(),
            bucket,
            units,
            fingerprint: format!(
                "{}:{}:{}",
                self.account, self.snapshot.quantity, self.snapshot.cents
            ),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum State {
    Held,
    Committed,
    Released,
    Rejected,
}
impl State {
    fn name(self) -> &'static str {
        match self {
            Self::Held => "held",
            Self::Committed => "committed",
            Self::Released => "released",
            Self::Rejected => "rejected",
        }
    }
    fn parse(value: &str) -> rusqlite::Result<Self> {
        match value {
            "held" => Ok(Self::Held),
            "committed" => Ok(Self::Committed),
            "released" => Ok(Self::Released),
            "rejected" => Ok(Self::Rejected),
            _ => Err(rusqlite::Error::InvalidQuery),
        }
    }
    pub(crate) fn wire(self) -> wire::ResourceState {
        match self {
            Self::Held => wire::ResourceState::Held,
            Self::Committed => wire::ResourceState::Committed,
            Self::Released => wire::ResourceState::Released,
            Self::Rejected => wire::ResourceState::Rejected,
        }
    }
    pub(crate) fn read(value: wire::ResourceState) -> Self {
        match value {
            wire::ResourceState::Held => Self::Held,
            wire::ResourceState::Committed => Self::Committed,
            wire::ResourceState::Released => Self::Released,
            wire::ResourceState::Rejected => Self::Rejected,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claim {
    pub key: String,
    pub bucket: String,
    pub units: u64,
    pub fingerprint: String,
}
impl Claim {
    pub(crate) fn write(&self, mut out: wire::claim::Builder<'_>) {
        out.set_key(&self.key);
        out.set_bucket(&self.bucket);
        out.set_units(self.units);
        out.set_fingerprint(&self.fingerprint);
    }
    pub(crate) fn read(input: wire::claim::Reader<'_>) -> crate::capnp::Result<Self> {
        Ok(Self {
            key: input.get_key()?.to_str()?.into(),
            bucket: input.get_bucket()?.to_str()?.into(),
            units: input.get_units(),
            fingerprint: input.get_fingerprint()?.to_str()?.into(),
        })
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub claim: Claim,
    pub state: State,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bucket {
    pub name: String,
    pub initial: u64,
    pub available: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Audit {
    pub buckets: Vec<Bucket>,
    pub entries: Vec<Entry>,
}
fn unsigned(row: &rusqlite::Row<'_>, index: usize) -> rusqlite::Result<u64> {
    let value: i64 = row.get(index)?;
    u64::try_from(value).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(
            index,
            rusqlite::types::Type::Integer,
            Box::new(e),
        )
    })
}
fn open(path: &Path) -> Result<Connection> {
    let connection = Connection::open(path)?;
    connection.busy_timeout(std::time::Duration::from_secs(2))?;
    connection.execute_batch(
        "PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON;
        CREATE TABLE IF NOT EXISTS faults(id TEXT PRIMARY KEY);",
    )?;
    Ok(connection)
}
pub(crate) fn take_fault(connection: &Connection, id: &str) -> rusqlite::Result<bool> {
    Ok(connection.execute("INSERT OR IGNORE INTO faults VALUES(?1)", [id])? == 1)
}
pub(crate) struct Resources(pub Connection);
impl Resources {
    pub fn open(path: &Path, buckets: &[(String, u64)]) -> Result<Self> {
        let mut connection = open(path)?;
        connection.execute_batch("CREATE TABLE IF NOT EXISTS buckets(name TEXT PRIMARY KEY, initial INTEGER NOT NULL CHECK(initial>=0), available INTEGER NOT NULL CHECK(available>=0 AND available<=initial));
          CREATE TABLE IF NOT EXISTS entries(key TEXT PRIMARY KEY, bucket TEXT NOT NULL REFERENCES buckets(name), units INTEGER NOT NULL CHECK(units>0), fingerprint TEXT NOT NULL, state TEXT NOT NULL CHECK(state IN ('held','committed','released','rejected')));")?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        for (name, initial) in buckets {
            let value = i64::try_from(*initial)?;
            tx.execute(
                "INSERT OR IGNORE INTO buckets VALUES(?1,?2,?2)",
                params![name, value],
            )?;
            let actual: i64 =
                tx.query_row("SELECT initial FROM buckets WHERE name=?1", [name], |r| {
                    r.get(0)
                })?;
            if actual != value {
                return Err("configured initial balance changed on restart".into());
            }
        }
        tx.commit()?;
        Ok(Self(connection))
    }
    pub fn apply(&mut self, method: &str, claim: &Claim) -> crate::capnp::Result<State> {
        if claim.units == 0
            || claim.units > i64::MAX as u64
            || claim.key.len() > 130
            || claim.fingerprint.len() > 128
        {
            return Err(error("invalid: claim"));
        }
        self.apply_sql(method, claim)
            .map_err(|e| error(format!("resource storage: {e}")))
    }
    fn apply_sql(&mut self, method: &str, claim: &Claim) -> rusqlite::Result<State> {
        let units = i64::try_from(claim.units)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?;
        let tx = self
            .0
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let existing = tx
            .query_row(
                "SELECT bucket,units,fingerprint,state FROM entries WHERE key=?1",
                [&claim.key],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        unsigned(r, 1)?,
                        r.get::<_, String>(2)?,
                        State::parse(&r.get::<_, String>(3)?)?,
                    ))
                },
            )
            .optional()?;
        let state = if let Some((bucket, existing_units, fingerprint, state)) = existing {
            if (bucket, existing_units, fingerprint)
                != (claim.bucket.clone(), claim.units, claim.fingerprint.clone())
            {
                return Err(rusqlite::Error::InvalidParameterName(
                    "conflict: claim identity changed".into(),
                ));
            }
            match (method, state) {
                ("commit", State::Held) => State::Committed,
                ("release", State::Held | State::Committed) => {
                    tx.execute(
                        "UPDATE buckets SET available=available+?1 WHERE name=?2",
                        params![units, claim.bucket],
                    )?;
                    State::Released
                }
                ("release", State::Rejected) => State::Released,
                _ => state,
            }
        } else if method == "release" {
            // Permanent tombstone: a delayed reserve must never resurrect a cancellation.
            State::Released
        } else if method == "reserve" {
            if tx.execute(
                "UPDATE buckets SET available=available-?1 WHERE name=?2 AND available>=?1",
                params![units, claim.bucket],
            )? == 1
            {
                State::Held
            } else {
                State::Rejected
            }
        } else {
            return Err(rusqlite::Error::InvalidQuery);
        };
        tx.execute("INSERT INTO entries VALUES(?1,?2,?3,?4,?5) ON CONFLICT(key) DO UPDATE SET state=excluded.state",params![claim.key,claim.bucket,units,claim.fingerprint,state.name()])?;
        tx.commit()?;
        Ok(state)
    }
    pub fn audit(&self) -> crate::capnp::Result<Audit> {
        let read = || -> rusqlite::Result<Audit> {
            let buckets = self
                .0
                .prepare("SELECT name,initial,available FROM buckets ORDER BY name")?
                .query_map([], |r| {
                    Ok(Bucket {
                        name: r.get(0)?,
                        initial: unsigned(r, 1)?,
                        available: unsigned(r, 2)?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let entries = self
                .0
                .prepare("SELECT key,bucket,units,fingerprint,state FROM entries ORDER BY key")?
                .query_map([], |r| {
                    Ok(Entry {
                        claim: Claim {
                            key: r.get(0)?,
                            bucket: r.get(1)?,
                            units: unsigned(r, 2)?,
                            fingerprint: r.get(3)?,
                        },
                        state: State::parse(&r.get::<_, String>(4)?)?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(Audit { buckets, entries })
        };
        read().map_err(|e| error(format!("audit storage: {e}")))
    }
}
pub(crate) struct Orders(pub Connection);
fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<OrderRow> {
    let intent: String = r.get(4)?;
    let state: String = r.get(5)?;
    let status = match state.as_str() {
        "confirmed" => Status::Confirmed,
        "cancelled" => Status::Cancelled,
        "declined" => Status::Declined,
        "pending" if intent == "cancel" => Status::Cancelling,
        "pending" => Status::Processing,
        _ => return Err(rusqlite::Error::InvalidQuery),
    };
    Ok(OrderRow {
        account: r.get(0)?,
        snapshot: Snapshot {
            key: r.get(1)?,
            quantity: r.get(2)?,
            cents: unsigned(r, 3)?,
            status,
            revision: unsigned(r, 6)?,
        },
        intent,
    })
}
impl Orders {
    pub fn open(path: &Path) -> Result<Self> {
        let c = open(path)?;
        c.execute_batch("CREATE TABLE IF NOT EXISTS orders(account TEXT NOT NULL, key TEXT NOT NULL, quantity INTEGER NOT NULL, cents INTEGER NOT NULL, intent TEXT NOT NULL CHECK(intent IN ('buy','cancel','reject')), state TEXT NOT NULL CHECK(state IN ('pending','confirmed','cancelled','declined')), revision INTEGER NOT NULL DEFAULT 0, PRIMARY KEY(account,key));")?;
        Ok(Self(c))
    }
    pub fn get(&self, account: &str, key: &str) -> crate::capnp::Result<OrderRow> {
        self.0
            .query_row(
                "SELECT * FROM orders WHERE account=?1 AND key=?2",
                params![account, key],
                row,
            )
            .optional()
            .map_err(|e| error(e.to_string()))?
            .ok_or_else(|| error("not found: order"))
    }
    pub fn place(
        &mut self,
        account: &str,
        key: &str,
        quantity: u32,
    ) -> crate::capnp::Result<OrderRow> {
        if !valid_key(key) {
            return Err(error("invalid: request key"));
        }
        let cents = quote(quantity)?;
        self.0.execute("INSERT OR IGNORE INTO orders(account,key,quantity,cents,intent,state) VALUES(?1,?2,?3,?4,'buy','pending')",params![account,key,quantity,cents as i64]).map_err(|e|error(e.to_string()))?;
        let row = self.get(account, key)?;
        if row.snapshot.quantity != 0 && row.snapshot.quantity != quantity {
            return Err(error(
                "conflict: request key already bound to another quantity",
            ));
        }
        Ok(row)
    }
    pub fn cancel(&mut self, account: &str, key: &str) -> crate::capnp::Result<OrderRow> {
        if !valid_key(key) {
            return Err(error("invalid: request key"));
        }
        let mut op = || -> rusqlite::Result<()> {
            // This method runs without an await; the local transaction also makes restart atomic.
            let tx = self
                .0
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            tx.execute("INSERT OR IGNORE INTO orders(account,key,quantity,cents,intent,state) VALUES(?1,?2,0,0,'cancel','cancelled')",params![account,key])?;
            tx.execute("UPDATE orders SET intent='cancel',revision=revision+1 WHERE account=?1 AND key=?2 AND state='pending' AND intent!='cancel'",params![account,key])?;
            tx.commit()
        };
        op().map_err(|e| error(e.to_string()))?;
        self.get(account, key)
    }
    pub fn reject(&mut self, account: &str, key: &str) -> crate::capnp::Result<()> {
        self.0.execute("UPDATE orders SET intent='reject',revision=revision+1 WHERE account=?1 AND key=?2 AND state='pending' AND intent='buy'",params![account,key]).map_err(|e|error(e.to_string()))?;
        Ok(())
    }
    pub fn finish(&mut self, account: &str, key: &str, intent: &str) -> crate::capnp::Result<()> {
        let state = match intent {
            "buy" => "confirmed",
            "cancel" => "cancelled",
            "reject" => "declined",
            _ => return Err(error("invalid: intent")),
        };
        // A concurrent cancel wins if and only if it commits before this confirmation CAS.
        self.0.execute("UPDATE orders SET state=?1,revision=revision+1 WHERE account=?2 AND key=?3 AND state='pending' AND intent=?4",params![state,account,key,intent]).map_err(|e|error(e.to_string()))?;
        Ok(())
    }
    pub fn list(
        &self,
        account: Option<&str>,
        pending: bool,
    ) -> crate::capnp::Result<Vec<OrderRow>> {
        let mut statement=self.0.prepare("SELECT * FROM orders WHERE (?1 IS NULL OR account=?1) AND (?2=0 OR state='pending') ORDER BY account,key").map_err(|e|error(e.to_string()))?;
        let rows = statement
            .query_map(params![account, pending], row)
            .map_err(|e| error(e.to_string()))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|e| error(e.to_string()))
    }
}
