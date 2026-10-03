//! Native network Join profile: independently routed XOR shares and mutual MACs.
//! RPC endpoints are still authenticated by the pinned Native session. The Join
//! proof additionally binds all shares to one opaque object at that endpoint.
use super::{failed, gone, random, Route, State, VatId};
use capnp::{any_pointer, message::Builder, private::capability::ResponseHook, Error};
use capnp_rpc::{
    multiparty::{JoinDestination, JoinNetwork, JoinSession},
    third_party::ThirdPartyExchange,
};
use ring::hmac;
use std::{
    any::Any,
    cell::RefCell,
    collections::{HashMap, HashSet},
    rc::{Rc, Weak},
};
use zeroize::{Zeroize, Zeroizing};

const LIMIT: usize = 4096;
const HOPS: u16 = 64;
const PART: &[u8; 4] = b"RJK1";
const RESULT: &[u8; 4] = b"RJR1";
const ACCEPT: &[u8; 4] = b"RJA1";
type Operation = (VatId, [u8; 32]);
type GroupKey = (Operation, (usize, usize));

fn message(bytes: &[u8]) -> capnp::Result<Builder<capnp::message::HeapAllocator>> {
    let mut result = Builder::new_default();
    result.set_root::<capnp::data::Owned>(bytes)?;
    Ok(result)
}
fn bytes<const N: usize>(
    reader: any_pointer::Reader<'_>,
    magic: &[u8; 4],
) -> capnp::Result<[u8; N]> {
    let bytes: [u8; N] = reader
        .get_as::<capnp::data::Reader>()?
        .try_into()
        .map_err(|_| failed("invalid Native Join length"))?;
    if &bytes[..4] != magic {
        return Err(failed("invalid Native Join profile"));
    }
    Ok(bytes)
}
fn word(b: &[u8]) -> u16 {
    u16::from_be_bytes(b.try_into().unwrap())
}
fn array(b: &[u8]) -> [u8; 32] {
    b.try_into().unwrap()
}

struct Part {
    operation: Operation,
    count: u16,
    index: u16,
    share: Zeroizing<[u8; 32]>,
    hops: u16,
}
impl Part {
    fn read(reader: any_pointer::Reader<'_>) -> capnp::Result<Self> {
        let b = Zeroizing::new(bytes::<106>(reader, PART)?);
        let part = Self {
            operation: (array(&b[4..36]), array(&b[36..68])),
            count: word(&b[68..70]),
            index: word(&b[70..72]),
            share: Zeroizing::new(array(&b[72..104])),
            hops: word(&b[104..106]),
        };
        if part.count == 0 || part.index >= part.count || part.hops > HOPS {
            return Err(failed("invalid Native Join part"));
        }
        Ok(part)
    }
    fn write(&self) -> capnp::Result<Builder<capnp::message::HeapAllocator>> {
        let mut b = Zeroizing::new(Vec::with_capacity(106));
        b.extend(PART);
        b.extend(self.operation.0);
        b.extend(self.operation.1);
        b.extend(self.count.to_be_bytes());
        b.extend(self.index.to_be_bytes());
        b.extend(*self.share);
        b.extend(self.hops.to_be_bytes());
        message(&b)
    }
}

struct ResultPart {
    operation: Operation,
    count: u16,
    index: u16,
    host: VatId,
    group: [u8; 32],
    ordinal: u16,
    proof: Option<[u8; 32]>,
}
impl ResultPart {
    fn read(reader: any_pointer::Reader<'_>) -> capnp::Result<Self> {
        let b = bytes::<171>(reader, RESULT)?;
        if b[170] > 1 || b[170] == 0 && b[138..170] != [0; 32] {
            return Err(failed("invalid Native Join proof flag"));
        }
        Ok(Self {
            operation: (array(&b[4..36]), array(&b[36..68])),
            count: word(&b[68..70]),
            index: word(&b[70..72]),
            host: array(&b[72..104]),
            group: array(&b[104..136]),
            ordinal: word(&b[136..138]),
            proof: (b[170] == 1).then(|| array(&b[138..170])),
        })
    }
    fn write(&self) -> capnp::Result<Builder<capnp::message::HeapAllocator>> {
        let mut b = Vec::with_capacity(171);
        b.extend(RESULT);
        b.extend(self.operation.0);
        b.extend(self.operation.1);
        b.extend(self.count.to_be_bytes());
        b.extend(self.index.to_be_bytes());
        b.extend(self.host);
        b.extend(self.group);
        b.extend(self.ordinal.to_be_bytes());
        b.extend(self.proof.unwrap_or_default());
        b.push(u8::from(self.proof.is_some()));
        message(&b)
    }
}

fn transcript(
    role: &[u8],
    operation: Operation,
    count: u16,
    host: VatId,
    group: [u8; 32],
) -> Vec<u8> {
    let mut data = b"ReProto Native Join v1\0".to_vec();
    data.extend(role);
    data.extend(operation.0);
    data.extend(operation.1);
    data.extend(count.to_be_bytes());
    data.extend(host);
    data.extend(group);
    data
}
fn mac(key: &[u8], data: &[u8]) -> [u8; 32] {
    array(hmac::sign(&hmac::Key::new(hmac::HMAC_SHA256, key), data).as_ref())
}
fn verify(key: &[u8], data: &[u8], proof: &[u8]) -> capnp::Result<()> {
    hmac::verify(&hmac::Key::new(hmac::HMAC_SHA256, key), data, proof)
        .map_err(|_| failed("Native Join authentication failed"))
}

struct Group {
    operation: Operation,
    count: u16,
    host: VatId,
    id: [u8; 32],
    seen: HashSet<u16>,
    key: Zeroizing<[u8; 32]>,
    value: Rc<ThirdPartyExchange>,
    live: usize,
    accepted: bool,
    canceled: bool,
}
#[derive(Default)]
pub(super) struct Table {
    groups: HashMap<GroupKey, Weak<RefCell<Group>>>,
    by_id: HashMap<[u8; 32], Weak<RefCell<Group>>>,
    retired: HashSet<Operation>,
    live_parts: usize,
    contributed: u64,
    accepted: u64,
}
#[derive(Clone, Copy, Debug)]
pub struct Stats {
    pub groups: usize,
    pub parts: usize,
    pub retired: usize,
    pub contributed: u64,
    pub accepted: u64,
}
impl Table {
    pub(super) fn stats(&self) -> Stats {
        Stats {
            groups: self.groups.len(),
            parts: self.live_parts,
            retired: self.retired.len(),
            contributed: self.contributed,
            accepted: self.accepted,
        }
    }
}
struct Guard {
    table: Weak<RefCell<Table>>,
    key: GroupKey,
    group: Rc<RefCell<Group>>,
}
impl Drop for Guard {
    fn drop(&mut self) {
        let Some(table) = self.table.upgrade() else {
            return;
        };
        let (last, id, operation) = {
            let mut g = self.group.borrow_mut();
            g.live -= 1;
            if !g.accepted {
                g.canceled = true;
                g.key.zeroize();
            }
            (g.live == 0, g.id, g.operation)
        };
        let mut table = table.borrow_mut();
        table.live_parts -= 1;
        table.retired.insert(operation);
        if last {
            table.groups.remove(&self.key);
            table.by_id.remove(&id);
        }
    }
}

pub(super) struct Profile(pub(super) Weak<State>);
impl Profile {
    fn state(&self) -> capnp::Result<Rc<State>> {
        self.0
            .upgrade()
            .filter(|s| !s.closed.get())
            .ok_or_else(gone)
    }
}
struct Session {
    state: Weak<State>,
    operation: Operation,
    shares: Vec<Zeroizing<[u8; 32]>>,
    key: Zeroizing<[u8; 32]>,
}
impl Session {
    fn new(state: &Rc<State>, count: u16) -> capnp::Result<Self> {
        if count as usize > LIMIT {
            return Err(Error::overloaded("Native Join part limit".into()));
        }
        if count == 0 {
            return Err(failed("empty Native Join"));
        }

        let mut key = Zeroizing::new([0; 32]);
        let mut shares = Vec::with_capacity(count.into());
        for _ in 0..count {
            let share = Zeroizing::new(random()?);
            for (k, s) in key.iter_mut().zip(share.iter()) {
                *k ^= s;
            }
            shares.push(share);
        }
        Ok(Session {
            state: Rc::downgrade(state),
            operation: (state.local, random()?),
            shares,
            key,
        })
    }
}
impl Session {
    fn prepare(
        &self,
        responses: &[Box<dyn ResponseHook>],
    ) -> capnp::Result<Option<(VatId, Builder<capnp::message::HeapAllocator>)>> {
        if responses.len() != self.shares.len() {
            return Err(failed("missing Native Join responses"));
        }
        let results: Vec<_> = responses
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let r = ResultPart::read(r.get()?)?;
                if r.operation != self.operation
                    || r.count as usize != responses.len()
                    || r.index as usize != i
                    || r.ordinal >= r.count
                {
                    return Err(failed("inconsistent Native Join response"));
                }
                Ok(r)
            })
            .collect::<capnp::Result<_>>()?;
        let first = &results[0];
        if results
            .iter()
            .any(|r| r.host != first.host || r.group != first.group)
        {
            return Ok(None);
        }
        let ordinals: HashSet<_> = results.iter().map(|r| r.ordinal).collect();
        if ordinals.len() != results.len() {
            return Err(failed("duplicate Native Join result ordinal"));
        }
        let proofs: Vec<_> = results.iter().filter_map(|r| r.proof).collect();
        if proofs.len() != 1 {
            return Err(failed("missing or duplicate Native Join host proof"));
        }
        verify(
            &self.key[..],
            &transcript(
                b"host",
                self.operation,
                first.count,
                first.host,
                first.group,
            ),
            &proofs[0],
        )?;
        let proof = mac(
            &self.key[..],
            &transcript(
                b"caller",
                self.operation,
                first.count,
                first.host,
                first.group,
            ),
        );
        let mut completion = Vec::with_capacity(132);
        completion.extend(ACCEPT);
        completion.extend(self.operation.0);
        completion.extend(self.operation.1);
        completion.extend(first.group);
        completion.extend(proof);
        Ok(Some((first.host, message(&completion)?)))
    }
}
impl JoinNetwork<VatId> for Profile {
    fn start(&self, count: u16) -> capnp::Result<Box<dyn JoinSession<VatId>>> {
        Ok(Box::new(Session::new(&self.state()?, count)?))
    }

    fn forward(
        &self,
        part: any_pointer::Reader<'_>,
    ) -> capnp::Result<Builder<capnp::message::HeapAllocator>> {
        let mut part = Part::read(part)?;
        if part.hops == 0 {
            return Err(failed("Native Join forwarding depth exceeded"));
        }
        part.hops -= 1;
        part.write()
    }
    fn contribute(
        &self,
        part: any_pointer::Reader<'_>,
        identity: (usize, usize),
        value: Rc<ThirdPartyExchange>,
    ) -> capnp::Result<(Builder<capnp::message::HeapAllocator>, Box<dyn Any>)> {
        let state = self.state()?;
        let part = Part::read(part)?;
        let key = (part.operation, identity);
        let mut table = state.joins.borrow_mut();
        if table.retired.contains(&part.operation) {
            return Err(failed("retired Native Join"));
        }
        if table.live_parts >= LIMIT {
            return Err(Error::overloaded("Native Join part limit".into()));
        }
        let group = match table.groups.get(&key).and_then(Weak::upgrade) {
            Some(group) => group,
            None => {
                if table.groups.len() + table.retired.len() >= LIMIT {
                    return Err(Error::overloaded("Native Join group limit".into()));
                }
                let id = loop {
                    let id = random()?;
                    if !table.by_id.contains_key(&id) {
                        break id;
                    }
                };
                let group = Rc::new(RefCell::new(Group {
                    operation: part.operation,
                    count: part.count,
                    host: state.local,
                    id,
                    seen: HashSet::new(),
                    key: Zeroizing::new([0; 32]),
                    value,
                    live: 0,
                    accepted: false,
                    canceled: false,
                }));
                table.groups.insert(key, Rc::downgrade(&group));
                table.by_id.insert(id, Rc::downgrade(&group));
                group
            }
        };
        let mut g = group.borrow_mut();
        if g.count != part.count || g.canceled || g.accepted || g.seen.contains(&part.index) {
            return Err(failed("duplicate or inconsistent Native Join part"));
        }
        let ordinal = g.seen.len() as u16;
        // Construct the result before committing a new share; serialization
        // failure must not leave a group with an unowned registration.
        let mut combined = Zeroizing::new(*g.key);
        for (k, s) in combined.iter_mut().zip(part.share.iter()) {
            *k ^= s;
        }
        let proof = (ordinal + 1 == g.count).then(|| {
            mac(
                &combined[..],
                &transcript(b"host", g.operation, g.count, g.host, g.id),
            )
        });
        let result = ResultPart {
            operation: g.operation,
            count: g.count,
            index: part.index,
            host: g.host,
            group: g.id,
            ordinal,
            proof,
        }
        .write()?;
        g.key = combined;
        g.seen.insert(part.index);
        g.live += 1;
        table.live_parts += 1;
        table.contributed += 1;
        drop(g);
        drop(table);
        Ok((
            result,
            Box::new(Guard {
                table: Rc::downgrade(&state.joins),
                key,
                group,
            }),
        ))
    }
}
impl JoinSession<VatId> for Session {
    fn part(&self, index: u16) -> capnp::Result<Builder<capnp::message::HeapAllocator>> {
        let share = self
            .shares
            .get(index as usize)
            .ok_or_else(|| failed("Join part index out of range"))?;
        Part {
            operation: self.operation,
            count: self.shares.len() as u16,
            index,
            share: Zeroizing::new(**share),
            hops: HOPS,
        }
        .write()
    }
    fn finish(
        self: Box<Self>,
        responses: &[Box<dyn ResponseHook>],
    ) -> capnp::Result<Option<JoinDestination<VatId>>> {
        let state = self
            .state
            .upgrade()
            .filter(|s| !s.closed.get())
            .ok_or_else(gone)?;
        let Some((host, completion)) = self.prepare(responses)? else {
            return Ok(None);
        };
        if host == state.local {
            return Ok(Some(JoinDestination::Local(accept(
                &state,
                self.operation.0,
                completion.get_root_as_reader()?,
            )?)));
        }
        let connection = Box::new(Route(state.route(host)?));
        Ok(Some(JoinDestination::Remote {
            connection,
            completion,
        }))
    }
}

pub(super) fn is_completion(reader: any_pointer::Reader<'_>) -> bool {
    reader
        .get_as::<capnp::data::Reader>()
        .is_ok_and(|b| b.len() == 132 && b.starts_with(ACCEPT))
}
pub(super) fn accept(
    state: &Rc<State>,
    peer: VatId,
    reader: any_pointer::Reader<'_>,
) -> capnp::Result<Rc<ThirdPartyExchange>> {
    let b = bytes::<132>(reader, ACCEPT)?;
    let operation = (array(&b[4..36]), array(&b[36..68]));
    if peer != operation.0 {
        return Err(failed("Native Join caller identity mismatch"));
    }
    let mut table = state.joins.borrow_mut();
    if table.retired.contains(&operation) {
        return Err(failed("retired Native Join"));
    }
    let group = table
        .by_id
        .get(&array(&b[68..100]))
        .and_then(Weak::upgrade)
        .ok_or_else(|| failed("unknown Native Join group"))?;
    let mut g = group.borrow_mut();
    if g.operation != operation || g.accepted || g.canceled || g.seen.len() != g.count as usize {
        return Err(failed("incomplete or consumed Native Join"));
    }
    verify(
        &g.key[..],
        &transcript(b"caller", g.operation, g.count, g.host, g.id),
        &b[100..132],
    )?;
    table.accepted += 1;
    g.accepted = true;
    g.key.zeroize();
    Ok(g.value.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_rpc::Network;
    use capntproto_test_support::runtime_test_capnp::harness;

    struct Echo;
    impl harness::Server for Echo {}
    #[derive(Clone)]
    struct Reply(Rc<Builder<capnp::message::HeapAllocator>>);
    impl ResponseHook for Reply {
        fn get(&self) -> capnp::Result<any_pointer::Reader<'_>> {
            self.0.get_root_as_reader()
        }
    }
    struct Fixture {
        networks: Vec<Network>,
        session: Session,
        caps: [harness::Client; 2],
        replies: Vec<Option<Reply>>,
        guards: Vec<Option<Box<dyn Any>>>,
        completion: Option<(VatId, Builder<capnp::message::HeapAllocator>)>,
        acquired: Option<Rc<ThirdPartyExchange>>,
        phase: u32,
        canceled: bool,
        scenario: String,
    }
    impl Fixture {
        fn new(count: u16, scenario: &str) -> Self {
            let networks: Vec<_> = [1, 4, 5].map(|n| Network::new([n; 32]).0).into();
            let session = Session::new(&networks[0].state, count).unwrap();
            Self {
                networks,
                session,
                caps: [capnp_rpc::new_client(Echo), capnp_rpc::new_client(Echo)],
                replies: (0..count).map(|_| None).collect(),
                guards: (0..count).map(|_| None).collect(),
                completion: None,
                acquired: None,
                phase: 0,
                canceled: false,
                scenario: scenario.into(),
            }
        }
        fn destination(&self, i: usize) -> (usize, usize) {
            let last = i + 1 == self.replies.len();
            (
                if self.scenario == "host" && last {
                    2
                } else {
                    1
                },
                usize::from(self.scenario == "object" && last),
            )
        }
        fn arrive(&mut self, i: usize) {
            let message = self.session.part(i as u16).unwrap();
            let mut part = Part::read(message.get_root_as_reader().unwrap()).unwrap();
            if self.scenario == "tampered" && i == 0 {
                part.share[0] ^= 1;
            }
            let message = part.write().unwrap();
            let (host, target) = self.destination(i);
            let cap = &self.caps[target].client;
            let identity = (cap.hook.get_brand(), cap.hook.get_ptr());
            let profile = Profile(Rc::downgrade(&self.networks[host].state));
            let (response, guard) = profile
                .contribute(
                    message.get_root_as_reader().unwrap(),
                    identity,
                    ThirdPartyExchange::from_capability(cap.clone()),
                )
                .unwrap();
            self.replies[i] = Some(Reply(Rc::new(response)));
            self.guards[i] = Some(guard);
        }
        fn responses(&self) -> Vec<Box<dyn ResponseHook>> {
            self.replies
                .iter()
                .map(|r| Box::new(r.as_ref().unwrap().clone()) as Box<dyn ResponseHook>)
                .collect()
        }
        fn decide(&mut self) {
            match self.session.prepare(&self.responses()) {
                Ok(Some(c)) => {
                    self.completion = Some(c);
                    self.phase = 1;
                }
                Ok(None) => self.phase = 3,
                Err(_) => self.phase = 5,
            }
        }
        fn acquire(&mut self) -> capnp::Result<()> {
            let (host, completion) = self.completion.as_ref().unwrap();
            let state = &self
                .networks
                .iter()
                .find(|n| n.state.local == *host)
                .unwrap()
                .state;
            self.acquired = Some(accept(
                state,
                self.networks[0].state.local,
                completion.get_root_as_reader()?,
            )?);
            self.phase = 2;
            Ok(())
        }
        fn release(&mut self, i: usize) {
            self.guards[i].take();
        }
        fn parts(&self) -> usize {
            self.networks
                .iter()
                .map(|n| n.state.joins.borrow().live_parts)
                .sum()
        }
        fn groups(&self) -> usize {
            self.networks
                .iter()
                .map(|n| n.state.joins.borrow().groups.len())
                .sum()
        }
        fn accepted(&self) -> u64 {
            self.networks
                .iter()
                .map(|n| n.state.joins.borrow().accepted)
                .sum()
        }
    }

    #[tokio::test(flavor = "current_thread")]
    async fn proofs_require_every_share_and_bind_object_peer_and_role() {
        tokio::task::LocalSet::new()
            .run_until(async {
                for scenario in ["equal", "object", "host", "tampered"] {
                    for order in [[0, 1, 2], [2, 0, 1], [1, 2, 0]] {
                        let mut f = Fixture::new(3, scenario);
                        for i in order {
                            f.arrive(i);
                        }
                        f.decide();
                        assert_eq!(
                            f.phase,
                            match scenario {
                                "equal" => 1,
                                "tampered" => 5,
                                _ => 3,
                            }
                        );
                        if f.phase == 1 {
                            let (_, completion) = f.completion.as_ref().unwrap();
                            let state = &f.networks[1].state;
                            assert!(accept(
                                state,
                                [9; 32],
                                completion.get_root_as_reader().unwrap()
                            )
                            .is_err());
                            let mut reflected =
                                bytes::<132>(completion.get_root_as_reader().unwrap(), ACCEPT)
                                    .unwrap();
                            let proof = f
                                .responses()
                                .iter()
                                .find_map(|r| ResultPart::read(r.get().unwrap()).unwrap().proof)
                                .unwrap();
                            reflected[100..].copy_from_slice(&proof);
                            assert!(accept(
                                state,
                                [1; 32],
                                message(&reflected).unwrap().get_root_as_reader().unwrap()
                            )
                            .is_err());
                            f.acquire().unwrap();
                            assert!(f.acquire().is_err());
                            assert_eq!(f.accepted(), 1);
                        }
                        for i in 0..3 {
                            f.release(i);
                        }
                        assert_eq!((f.parts(), f.groups()), (0, 0));
                    }
                }
                let mut f = Fixture::new(2, "equal");
                f.arrive(0);
                f.arrive(1);
                f.decide();
                f.release(0);
                assert!(
                    f.acquire().is_err(),
                    "Finish must cancel an unaccepted Join"
                );
                assert_eq!(f.accepted(), 0);
                f.release(1);
                assert_eq!(f.groups(), 0);
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn malformed_duplicate_replayed_and_exhausted_parts_are_rejected() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let mut f = Fixture::new(2, "equal");
                f.arrive(0);
                let profile = Profile(Rc::downgrade(&f.networks[1].state));
                let message = f.session.part(0).unwrap();
                let identity = (
                    f.caps[0].client.hook.get_brand(),
                    f.caps[0].client.hook.get_ptr(),
                );
                let value = ThirdPartyExchange::from_capability(f.caps[0].client.clone());
                assert!(profile
                    .contribute(
                        message.get_root_as_reader().unwrap(),
                        identity,
                        value.clone()
                    )
                    .is_err());
                let mut part = Part::read(message.get_root_as_reader().unwrap()).unwrap();
                part.count = 3;
                assert!(profile
                    .contribute(
                        part.write().unwrap().get_root_as_reader().unwrap(),
                        identity,
                        value.clone()
                    )
                    .is_err());
                part.count = 0;
                assert!(profile
                    .forward(part.write().unwrap().get_root_as_reader().unwrap())
                    .is_err());
                part.count = 2;
                part.hops = 0;
                assert!(profile
                    .forward(part.write().unwrap().get_root_as_reader().unwrap())
                    .is_err());
                f.release(0);
                let second = f.session.part(1).unwrap();
                assert!(profile
                    .contribute(second.get_root_as_reader().unwrap(), identity, value)
                    .is_err());
                assert_eq!(f.parts(), 0);
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_partial_host_cannot_forge_matching_results_or_reflect_a_proof() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let mut f = Fixture::new(2, "object");
                f.arrive(0);
                f.arrive(1);
                let mut first =
                    ResultPart::read(f.replies[0].as_ref().unwrap().get().unwrap()).unwrap();
                let mut second =
                    ResultPart::read(f.replies[1].as_ref().unwrap().get().unwrap()).unwrap();
                second.group = first.group;
                second.ordinal = 1;
                let part = f.session.part(0).unwrap();
                let part = Part::read(part.get_root_as_reader().unwrap()).unwrap();
                first.proof = Some(mac(
                    &part.share[..],
                    &transcript(b"host", part.operation, 2, first.host, first.group),
                ));
                let replies: Vec<Box<dyn ResponseHook>> = vec![
                    Box::new(Reply(Rc::new(first.write().unwrap()))),
                    Box::new(Reply(Rc::new(second.write().unwrap()))),
                ];
                assert!(f.session.prepare(&replies).is_err());
                assert_eq!(f.accepted(), 0);
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn quotas_bound_live_parts_and_retired_operations() {
        tokio::task::LocalSet::new()
            .run_until(async {
                let f = Fixture::new(1, "equal");
                assert!(Session::new(&f.networks[0].state, (LIMIT + 1) as u16).is_err());
                let profile = Profile(Rc::downgrade(&f.networks[1].state));
                let cap = &f.caps[0].client;
                let identity = (cap.hook.get_brand(), cap.hook.get_ptr());
                let mut guards = Vec::new();
                for _ in 0..LIMIT {
                    let session = Session::new(&f.networks[0].state, 1).unwrap();
                    let part = session.part(0).unwrap();
                    let (_, guard) = profile
                        .contribute(
                            part.get_root_as_reader().unwrap(),
                            identity,
                            ThirdPartyExchange::from_capability(cap.clone()),
                        )
                        .unwrap();
                    guards.push(guard);
                }
                assert_eq!(f.parts(), LIMIT);
                let session = Session::new(&f.networks[0].state, 1).unwrap();
                let part = session.part(0).unwrap();
                assert!(profile
                    .contribute(
                        part.get_root_as_reader().unwrap(),
                        identity,
                        ThirdPartyExchange::from_capability(cap.clone())
                    )
                    .is_err());
                drop(guards);
                assert_eq!((f.parts(), f.groups()), (0, 0));
                assert!(
                    profile
                        .contribute(
                            part.get_root_as_reader().unwrap(),
                            identity,
                            ThirdPartyExchange::from_capability(cap.clone())
                        )
                        .is_err(),
                    "retired nonces stay bounded instead of being forgotten"
                );
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn replay_tlc_multiparty_join_traces() {
        let path =
            capntproto_test_support::verification::input("CAPNTPROTO_MULTIPARTY_JOIN_TRACES")
                .expect("run this test through its verification driver");
        let cases: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        tokio::task::LocalSet::new()
            .run_until(async {
                for case in cases.as_array().unwrap() {
                    let n = case["parts"].as_u64().unwrap() as u16;
                    let mut f = Fixture::new(n, case["scenario"].as_str().unwrap());
                    for step in case["steps"].as_array().unwrap() {
                        let i = step["part"].as_u64().unwrap() as usize;
                        match step["action"].as_str().unwrap() {
                            "arrive" => f.arrive(i),
                            "decide" => f.decide(),
                            "accept" => f.acquire().unwrap(),
                            "cancel" => {
                                f.phase = 4;
                                f.canceled = true;
                            }
                            "finish" => f.release(i),
                            a => panic!("unknown action {a}"),
                        }
                        let state: Vec<u64> = step["state"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .map(|v| v.as_u64().unwrap())
                            .collect();
                        let arrived = f
                            .replies
                            .iter()
                            .enumerate()
                            .map(|(i, r)| if r.is_some() { 1 << i } else { 0 })
                            .sum::<u64>();
                        let released = f
                            .guards
                            .iter()
                            .zip(&f.replies)
                            .enumerate()
                            .map(|(i, (g, r))| {
                                if g.is_none() && r.is_some() {
                                    1 << i
                                } else {
                                    0
                                }
                            })
                            .sum::<u64>();
                        assert_eq!(
                            vec![
                                arrived,
                                released,
                                f.phase as u64,
                                f.parts() as u64,
                                f.groups() as u64,
                                f.accepted(),
                                u64::from(f.canceled)
                            ],
                            state,
                            "case {case} step {step}"
                        );
                    }
                }
            })
            .await;
    }
}
