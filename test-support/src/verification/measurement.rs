//! Per-check TLC evidence for the dedicated model job, including failed checks.
use super::*;

pub(super) struct Measurement {
    destination: Option<PathBuf>,
    log: PathBuf,
    started: Instant,
    value: serde_json::Value,
    finished: bool,
}
impl Measurement {
    pub fn start(module: &str, config: &str, exit: i32, log: &Path) -> Result<Self> {
        let identity = sha256(serde_json::to_vec(&(module, config, exit))?);
        let destination = std::env::var_os("CAPNTPROTO_TLC_REPORT")
            .map(|base| PathBuf::from(base).join(format!("{identity}.json")));
        let value = serde_json::json!({
            "format": 1, "module": module, "config_sha256": sha256(config),
            "expected_exit": exit, "passed": false, "seconds": null,
            "log_sha256": null,
        });
        let result = Self {
            destination,
            log: log.into(),
            started: Instant::now(),
            value,
            finished: false,
        };
        result.write(false)?;
        Ok(result)
    }
    fn write(&self, passed: bool) -> Result<()> {
        let Some(path) = &self.destination else {
            return Ok(());
        };
        fs::create_dir_all(path.parent().unwrap())?;
        let mut value = self.value.clone();
        value["passed"] = passed.into();
        value["seconds"] = self.started.elapsed().as_secs_f64().into();
        if self.finished {
            if let Ok(log) = fs::read(&self.log) {
                value["log_sha256"] = sha256(&log).into();
                fs::write(path.with_extension("log"), log)?;
            }
        }
        fs::write(path, serde_json::to_vec_pretty(&value)?)?;
        Ok(())
    }
    pub fn finish(mut self) -> Result<()> {
        self.finished = true;
        self.write(true)
    }
}
impl Drop for Measurement {
    fn drop(&mut self) {
        if !self.finished {
            self.finished = true;
            let _ = self.write(false);
        }
    }
}
