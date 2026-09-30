//! Exhaustive bounded graph of the production Rust revision/handoff guards.
use reproto::semantics::{HandoffPhase, HandoffState, Revision, Revisions};
use serde::Serialize;
use std::collections::{BTreeMap, VecDeque};
#[derive(Clone, Copy, Default, Eq, PartialEq, Ord, PartialOrd)]
struct State {
    v: Revisions,
    h: HandoffState,
}
impl State {
    fn vector(self) -> [u64; 8] {
        [
            self.v.head().get(),
            self.v.published().get(),
            match self.h.phase() {
                HandoffPhase::Proxying => 0,
                HandoffPhase::Offered => 1,
                HandoffPhase::Embargoed => 2,
                HandoffPhase::Direct => 3,
            },
            self.h.pending(),
            self.h.drained(),
            self.h.direct_calls(),
            u64::from(self.h.accepted()),
            u64::from(self.h.is_revoked()),
        ]
    }
}
#[derive(Serialize)]
struct Graph {
    states: Vec<[u64; 8]>,
    edges: Vec<(usize, usize)>,
}
fn main() {
    let bound = 2;
    let initial = State::default();
    let mut ids = BTreeMap::from([(initial, 0)]);
    let mut queue = VecDeque::from([initial]);
    let mut graph = Graph {
        states: vec![initial.vector()],
        edges: vec![],
    };
    while let Some(s) = queue.pop_front() {
        let from = ids[&s];
        let mut successors = vec![s]; // Explicit idle step, as in Spec = [][Next]_vars.
        if !s.h.is_revoked() {
            if s.v.head().get() < bound {
                successors.push(State {
                    v: s.v.stage(s.v.head()).unwrap(),
                    ..s
                });
            }
            for r in 1..=bound {
                if let Some(v) = s.v.publish(Revision::new(r), s.v.published()) {
                    successors.push(State { v, ..s });
                }
            }
        }
        for action in 0..7 {
            let mut n = s;
            let enabled = match action {
                0 => n.h.pending() + n.h.drained() < bound && n.h.enqueue(),
                1 => n.h.drain(),
                2 => n.h.provide(),
                3 => n.h.accept(true),
                4 => n.h.lift(),
                5 => n.h.direct_calls() < bound && n.h.invoke_direct(),
                _ => n.h.revoke(),
            };
            if enabled {
                successors.push(n);
            }
        }
        // Rejected inputs must leave the real transition state unchanged.
        let mut forged = s.h;
        assert!(!forged.accept(false));
        assert_eq!(forged, s.h);
        assert!(s.v.stage(s.v.head().checked_next().unwrap()).is_none());
        assert!(s
            .v
            .publish(s.v.head().checked_next().unwrap(), s.v.published())
            .is_none());
        for n in successors {
            let to = if let Some(id) = ids.get(&n) {
                *id
            } else {
                let id = graph.states.len();
                ids.insert(n, id);
                queue.push_back(n);
                graph.states.push(n.vector());
                id
            };
            graph.edges.push((from, to));
        }
    }
    println!("{}", serde_json::to_string(&graph).unwrap());
}
