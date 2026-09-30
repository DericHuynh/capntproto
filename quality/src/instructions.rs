//! Strict reader for the pinned Gungraun 0.20 / summary schema 7 output.
use crate::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Row {
    pub operation: String,
    pub payload_bytes: u64,
    pub instructions: u64,
    pub data_reads: u64,
    pub data_writes: u64,
}

pub fn parse(raw: &[u8]) -> Result<Vec<Row>> {
    let mut rows = BTreeMap::new();
    for line in std::str::from_utf8(raw)?
        .lines()
        .filter(|line| !line.trim().is_empty())
    {
        let value: Value = serde_json::from_str(line)?;
        let operation = value["function_name"]
            .as_str()
            .ok_or("missing instruction operation")?;
        let payload_bytes = match value["id"].as_str() {
            Some("small") => 64,
            Some("medium") => 1024,
            Some("large") => 65536,
            _ => return Err("unexpected instruction benchmark case".into()),
        };
        if value["version"] != "7"
            || value["group"] != "serialization"
            || !["decode", "encode"].contains(&operation)
            || value["module_path"] != format!("hot_paths::serialization::{operation}")
            || value["baselines"] != serde_json::json!([null, null])
        {
            return Err("unexpected instruction benchmark identity or stale baseline".into());
        }
        let profiles = value["profiles"]
            .as_array()
            .ok_or("missing instruction profiles")?;
        if profiles.len() != 1 || profiles[0]["tool"] != "Callgrind" {
            return Err("expected one Callgrind profile".into());
        }
        let metrics = &profiles[0]["data"]["total"]["metrics"];
        let metric = |name: &str| -> Result<u64> {
            let values = &metrics[name]["values"];
            if values.get("old").is_some() {
                return Err("unexpected previous-run counters".into());
            }
            values["new"]
                .as_u64()
                .ok_or_else(|| format!("missing/invalid {name} counter").into())
        };
        let row = Row {
            operation: operation.into(),
            payload_bytes,
            instructions: metric("Ir")?,
            data_reads: metric("Dr")?,
            data_writes: metric("Dw")?,
        };
        if row.instructions == 0
            || rows
                .insert((operation.to_owned(), payload_bytes), row)
                .is_some()
        {
            return Err("zero or duplicate instruction benchmark".into());
        }
    }
    if rows.len() != 6 {
        return Err("incomplete instruction benchmark matrix".into());
    }
    Ok(rows.into_values().collect())
}

#[cfg(test)]
pub(crate) fn fixture() -> Vec<Value> {
    // Schema-shaped fixtures, never published as measurements.
    let mut samples = Vec::new();
    for operation in ["decode", "encode"] {
        for id in ["small", "medium", "large"] {
            samples.push(serde_json::json!({"version":"7", "group":"serialization",
                    "function_name":operation,"module_path":format!("hot_paths::serialization::{operation}"),
                    "id":id,"baselines":[null,null],"profiles":[{"tool":"Callgrind","data":{"total":{"metrics":{
                        "Ir":{"values":{"new":100}},"Dr":{"values":{"new":20}},"Dw":{"values":{"new":0}}
                    }}}}]}));
        }
    }
    samples
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_missing_duplicate_stale_and_invalid_counters() {
        let mut samples = fixture();
        let raw = |samples: &[Value]| {
            samples
                .iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n")
        };
        assert_eq!(parse(raw(&samples).as_bytes()).unwrap().len(), 6);
        assert!(parse(raw(&samples[..5]).as_bytes()).is_err());
        let mut changed = samples.clone();
        changed[5] = changed[0].clone();
        assert!(parse(raw(&changed).as_bytes()).is_err());
        for bad in [serde_json::json!(0), serde_json::json!(-1), Value::Null] {
            let mut changed = samples.clone();
            changed[0]["profiles"][0]["data"]["total"]["metrics"]["Ir"]["values"]["new"] = bad;
            assert!(parse(raw(&changed).as_bytes()).is_err());
        }
        samples[0]["baselines"][0] = serde_json::json!("old");
        assert!(parse(raw(&samples).as_bytes()).is_err());
    }
}
