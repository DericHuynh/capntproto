//! Fresh scalar TLC graphs for native Rust edge-prefix replay.
use super::*;
use std::collections::VecDeque;

pub type State = BTreeMap<String, u64>;

/// Run TLC with invariants, then produce one reachable prefix ending at every
/// graph edge. This deliberately accepts only numeric scalar state variables;
/// unsupported labels fail instead of silently losing state information.
pub fn traces(module: &str, report: &str, config: &str) -> Result<Vec<Vec<State>>> {
    let graph = explore(module, report, config)?;
    let traces = graph.prefixes();
    eprintln!(
        "fresh TLC exploration: {} states, {} edge-prefix traces",
        graph.edges.len(),
        traces.len()
    );
    Ok(traces)
}

/// Run a fresh exploration, retaining every successor so callers can test
/// different histories through merged states and repeat modeled cycles.
pub fn explore(module: &str, report: &str, config: &str) -> Result<Graph> {
    let destination = root().join("target/verification").join(report);
    fs::create_dir_all(&destination)?;
    let _jvm_lock = lock(&root().join("target/verification/tlc.lock"))?;
    let (java, jar) = tools()?;
    let cfg = destination.join("model.cfg");
    fs::write(&cfg, config)?;
    let metadir = tempfile::tempdir_in(&destination)?;
    let dot = destination.join("graph.dot");
    let output = run(
        command(java)
            .arg(format!(
                "-DTLA-Library={}",
                root().join("verification").display()
            ))
            .args(["-XX:+UseParallelGC", "-Xmx1g", "-cp"])
            .arg(jar)
            .args(["tlc2.TLC", "-workers", "2", "-fp", "0", "-config"])
            .arg(cfg)
            .arg("-metadir")
            .arg(metadir.path())
            .args(["-dump", "dot"])
            .arg(&dot)
            .arg(module),
        &destination.join("tlc.log"),
        0,
    )?;
    if !output.contains("Model checking completed. No error has been found.") {
        return Err("TLC did not finish exploration".into());
    }
    parse(&fs::read_to_string(dot)?)
}

/// Canonical scalar graph: successor order depends on state values, not TLC's
/// worker schedule, DOT line order or numeric fingerprint IDs.
pub struct Graph {
    initial: State,
    edges: BTreeMap<State, Vec<State>>,
    digest: GraphDigest,
}
impl Graph {
    pub fn initial(&self) -> &State {
        &self.initial
    }
    pub fn successors(&self, state: &State) -> Result<&[State]> {
        self.edges
            .get(state)
            .map(Vec::as_slice)
            .ok_or_else(|| "state is outside the graph".into())
    }
    pub fn digest(&self) -> &GraphDigest {
        &self.digest
    }
    /// Empty histories are legal prefixes. Domain-specific replay must also
    /// require its intended completion condition to reject truncated evidence.
    pub fn validate(&self, history: &[State]) -> Result<()> {
        let mut prior = self.initial();
        for next in history {
            if !self.successors(prior)?.contains(next) {
                return Err("history contains an unmodeled transition".into());
            }
            prior = next;
        }
        Ok(())
    }
    fn prefixes(&self) -> Vec<Vec<State>> {
        let mut paths = BTreeMap::from([(self.initial.clone(), Vec::new())]);
        let mut queue = VecDeque::from([self.initial.clone()]);
        let mut traces = Vec::new();
        while let Some(from) = queue.pop_front() {
            let prefix = paths[&from].clone();
            for to in &self.edges[&from] {
                let mut path = prefix.clone();
                path.push(to.clone());
                if !paths.contains_key(to) {
                    paths.insert(to.clone(), path.clone());
                    queue.push_back(to.clone());
                }
                traces.push(path);
            }
        }
        traces
    }
}

fn parse(dot: &str) -> Result<Graph> {
    let mut nodes = BTreeMap::new();
    let mut edges: BTreeMap<i64, BTreeSet<i64>> = BTreeMap::new();
    let mut initial = BTreeSet::new();
    for line in dot.lines() {
        let Some((id, rest)) = line.split_once(' ') else {
            continue;
        };
        let Ok(id) = id.parse::<i64>() else { continue };
        if let Some(target) = rest.strip_prefix("-> ") {
            let target = target
                .split_whitespace()
                .next()
                .ok_or("missing edge target")?
                .parse()?;
            edges.entry(id).or_default().insert(target);
        } else if let Some(label) = rest.strip_prefix("[label=\"") {
            let (label, _) = label.split_once('"').ok_or("unterminated state")?;
            let mut state = State::new();
            for field in label.split("\\n") {
                let field = field
                    .trim()
                    .strip_prefix("/\\\\ ")
                    .ok_or("non-scalar state")?;
                let (key, value) = field.split_once(" = ").ok_or("invalid state field")?;
                if state.insert(key.to_owned(), value.parse()?).is_some() {
                    return Err("duplicate state field".into());
                }
            }
            if nodes.insert(id, state).is_some() {
                return Err("duplicate state ID".into());
            }
            if line.contains("style = filled") {
                initial.insert(id);
            }
        } else {
            return Err("unsupported TLC graph line".into());
        }
    }
    if initial.len() != 1 {
        return Err("replay requires exactly one initial state".into());
    }
    let start = *initial.first().unwrap();
    if edges.keys().any(|id| !nodes.contains_key(id)) {
        return Err("edge has no source state".into());
    }
    let fields: Vec<_> = nodes[&start].keys().collect();
    if nodes
        .values()
        .any(|s| s.keys().collect::<Vec<_>>() != fields)
    {
        return Err("state fields differ".into());
    }
    let mut reached = BTreeSet::from([start]);
    let mut queue = VecDeque::from([start]);
    while let Some(from) = queue.pop_front() {
        for to in edges.get(&from).into_iter().flatten() {
            if !nodes.contains_key(to) {
                return Err("edge has no destination state".into());
            }
            if reached.insert(*to) {
                queue.push_back(*to);
            }
        }
    }
    if reached.len() != nodes.len() || edges.values().all(BTreeSet::is_empty) {
        return Err("graph contains unreachable states or no transitions".into());
    }
    let mut graph = Graph {
        initial: nodes[&start].clone(),
        edges: BTreeMap::new(),
        digest: GraphDigest {
            states: nodes.len(),
            edges: edges.values().map(BTreeSet::len).sum(),
            sha256: String::new(),
        },
    };
    for (id, state) in &nodes {
        let mut next: Vec<_> = edges
            .get(id)
            .into_iter()
            .flatten()
            .map(|to| nodes[to].clone())
            .collect();
        next.sort();
        if graph.edges.insert(state.clone(), next).is_some() {
            return Err("duplicate scalar state under different IDs".into());
        }
    }
    graph.digest.sha256 = sha256(serde_json::to_vec(&(
        &graph.initial,
        graph.edges.iter().collect::<Vec<_>>(),
    ))?);
    Ok(graph)
}

/// Require specific invariant failures from named mutations, and optionally
/// check temporal properties with the model's stated fairness assumptions.
pub fn controls(
    module: &str,
    report: &str,
    config: &str,
    faults: &[(&str, &str)],
    live: Option<&str>,
) -> Result<()> {
    let mut checks: Vec<_> = faults
        .iter()
        .map(|(fault, invariant)| Check {
            id: (*fault).into(),
            module: module.into(),
            config: config.replace("Fault = \"none\"", &format!("Fault = \"{fault}\"")),
            exit: 12,
            message: format!("Invariant {invariant} is violated"),
            graph: None,
        })
        .collect();
    if let Some(live) = live {
        checks.push(Check {
            id: "liveness".into(),
            module: module.into(),
            config: live.into(),
            exit: 0,
            message: "Model checking completed. No error has been found.".into(),
            graph: None,
        });
    }
    verify(&Group {
        id: report.into(),
        report: format!("{report}/controls"),
        checks,
        inputs: BTreeMap::new(),
        scope: "bounded scalar exploration".into(),
        limits: serde_json::Value::Null,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state(n: u64) -> State {
        BTreeMap::from([("x".into(), n)])
    }
    fn source() -> String {
        let mut lines = Vec::new();
        for id in 0..4 {
            lines.push(format!(
                "{id} [label=\"/\\\\ x = {id}\"{}]",
                if id == 0 { ",style = filled" } else { "" }
            ));
        }
        for (from, to) in [(0, 1), (0, 2), (1, 3), (2, 3), (3, 1)] {
            lines.push(format!("{from} -> {to} [label=\"\"];"));
        }
        lines.join("\n")
    }
    #[test]
    fn preserves_merged_histories_cycles_and_canonical_order() {
        let graph = parse(&source()).unwrap();
        let reversed = source().lines().rev().collect::<Vec<_>>().join("\n");
        assert_eq!(graph.digest(), parse(&reversed).unwrap().digest());
        assert_eq!(graph.digest().edges, 5);
        assert_eq!(graph.prefixes().len(), 5);
        for first in [1, 2] {
            let history = vec![
                state(first),
                state(3),
                state(1),
                state(3),
                state(1),
                state(3),
            ];
            graph.validate(&history).unwrap();
            assert!(!graph.prefixes().contains(&history));
        }
        assert!(graph.validate(&[state(1), state(2)]).is_err());
        assert!(graph.successors(&state(99)).is_err());
    }
    #[test]
    fn rejects_incomplete_or_ambiguous_graphs() {
        for bad in [
            format!("{}\n8 -> 1 [label=\"\"];", source()),
            format!("{}\n1 -> 8 [label=\"\"];", source()),
            format!("{}\n8 [label=\"/\\\\ x = 8\"]", source()),
            format!(
                "{}\n8 [label=\"/\\\\ x = 1\"]\n1 -> 8 [label=\"\"];",
                source()
            ),
            source().replace("x = 3", "other = 3"),
            source().replace("x = 3", "x = TRUE"),
            source().replace(",style = filled", ""),
        ] {
            assert!(parse(&bad).is_err(), "accepted {bad}");
        }
    }
}
