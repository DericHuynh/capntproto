//! Reviewed integration histories retained when a local TLC model is replaced
//! by source proofs. These are regression inputs, never fresh model evidence.
use super::{exploration::State, *};

pub fn traces(name: &str) -> Result<Vec<Vec<State>>> {
    let catalog: serde_json::Value =
        serde_json::from_str(include_str!("../../regressions/catalog.json"))?;
    let case = catalog["cases"]
        .get(name)
        .ok_or("unknown regression corpus")?;
    // Resolve only exact catalog keys; callers cannot supply an arbitrary path.
    let data = fs::read(
        root()
            .join("test-support/regressions")
            .join(format!("{name}.json.gz")),
    )?;
    if sha256(&data) != case["sha256"] {
        return Err("regression corpus checksum mismatch".into());
    }
    let mut plain = Vec::new();
    GzDecoder::new(data.as_slice()).read_to_end(&mut plain)?;
    let paths: Vec<Vec<State>> = serde_json::from_slice(&plain)?;
    if paths.is_empty()
        || paths.len() as u64 != case["traces"]
        || paths.iter().any(Vec::is_empty)
        || paths.iter().map(Vec::len).sum::<usize>() as u64 != case["steps"]
    {
        return Err("incomplete regression corpus".into());
    }
    Ok(paths)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retired_models_retain_every_reviewed_history_and_required_source_proof() {
        let catalog: serde_json::Value =
            serde_json::from_str(include_str!("../../regressions/catalog.json")).unwrap();
        assert_eq!(catalog["format"], 1);
        let plan: serde_json::Value =
            serde_json::from_slice(&fs::read(root().join("verification/verus/plan.json")).unwrap())
                .unwrap();
        let functions = plan["functions"].as_array().unwrap();
        for (name, case) in catalog["cases"].as_object().unwrap() {
            traces(name).unwrap();
            assert!(!case["proofs"].as_array().unwrap().is_empty());
            for proof in case["proofs"].as_array().unwrap() {
                assert!(
                    functions.contains(proof),
                    "missing replacement proof: {proof}"
                );
            }
            let model = case["former_model"].as_str().unwrap();
            assert!(!root().join(format!("verification/{model}.tla")).exists());
        }
        assert!(traces("../outside").is_err());
    }
}
