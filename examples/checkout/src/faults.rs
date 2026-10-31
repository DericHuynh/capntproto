//! Opt-in process-test failpoints, absent from the business RPC interfaces.
use crate::{error, valid_key, Result};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FaultStage {
    Before,
    After,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FaultAction {
    Pause,
    Crash,
    Error,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FaultPlan {
    pub id: String,
    pub method: String,
    pub key: String,
    pub stage: FaultStage,
    pub action: FaultAction,
}
#[derive(Clone, Default)]
pub(crate) struct Faults {
    plans: Vec<FaultPlan>,
    directory: PathBuf,
}
impl Faults {
    pub fn load(path: Option<&Path>) -> Result<Self> {
        let Some(path) = path else {
            return Ok(Self::default());
        };
        let plans: Vec<FaultPlan> = serde_json::from_slice(&std::fs::read(path)?)?;
        let mut ids = std::collections::HashSet::new();
        for plan in &plans {
            if !valid_key(&plan.id) || !ids.insert(plan.id.clone()) {
                return Err("invalid or duplicate fault id".into());
            }
        }
        Ok(Self {
            plans,
            directory: path
                .parent()
                .ok_or("fault plan needs a directory")?
                .to_owned(),
        })
    }
    pub async fn at(
        &self,
        method: &str,
        stage: FaultStage,
        key: &str,
        consume: impl Fn(&str) -> crate::capnp::Result<bool>,
    ) -> crate::capnp::Result<Option<PathBuf>> {
        for plan in &self.plans {
            if plan.method != method
                || plan.stage != stage
                || plan.key != key
                || !consume(&plan.id)?
            {
                continue;
            }
            let file = |suffix: &str| self.directory.join(format!("{}.{}", plan.id, suffix));
            std::fs::write(file("hit"), b"hit").map_err(|e| error(e.to_string()))?;
            match plan.action {
                FaultAction::Crash => std::process::exit(86),
                FaultAction::Error => {
                    return Err(error("injected: reply unavailable; outcome is unknown"))
                }
                FaultAction::Pause => {
                    tokio::time::timeout(Duration::from_secs(30), async {
                        while !file("resume").exists() {
                            tokio::time::sleep(Duration::from_millis(5)).await;
                        }
                    })
                    .await
                    .map_err(|_| error("injected gate expired"))?;
                }
            }
            return Ok(Some(file("done")));
        }
        Ok(None)
    }
}
pub(crate) fn completed(path: Option<PathBuf>) -> crate::capnp::Result<()> {
    if let Some(path) = path {
        std::fs::write(path, b"done").map_err(|e| error(e.to_string()))?;
    }
    Ok(())
}
