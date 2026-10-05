//! Bounded child processes. File-backed output avoids pipe deadlocks.
use anyhow::{bail, Context, Result};
use std::{
    fs::File,
    io::{Read, Seek},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};
#[derive(Clone, Default)]
pub struct Cancellation(pub Arc<AtomicBool>);
impl Cancellation {
    pub fn install() -> Result<Self> {
        let token = Self::default();
        let t = token.clone();
        ctrlc::set_handler(move || t.0.store(true, Ordering::Relaxed))?;
        Ok(token)
    }
    pub fn cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}
struct Child(std::process::Child);
impl Drop for Child {
    fn drop(&mut self) {
        #[cfg(unix)]
        {
            let _ = nix::sys::signal::killpg(
                nix::unistd::Pid::from_raw(self.0.id() as i32),
                nix::sys::signal::Signal::SIGKILL,
            );
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
#[derive(Debug)]
pub struct Output {
    pub code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}
pub fn run(
    cmd: &mut Command,
    seconds: u64,
    log: Option<&std::path::Path>,
    cancel: &Cancellation,
) -> Result<Output> {
    if cancel.cancelled() {
        bail!("operation interrupted");
    }
    let mut stdout = tempfile::tempfile()?;
    let mut stderr = tempfile::tempfile()?;
    if let Some(path) = log {
        crate::write(path, [])?;
        let file = File::create(path)?;
        cmd.stdout(file.try_clone()?).stderr(file);
    } else {
        cmd.stdout(stdout.try_clone()?).stderr(stderr.try_clone()?);
    }
    cmd.stdin(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let mut child = Child(cmd.spawn().context("start child process")?);
    let start = Instant::now();
    let code = loop {
        if let Some(status) = child.0.try_wait()? {
            break status.code().unwrap_or(1);
        }
        if cancel.cancelled() {
            bail!("operation interrupted");
        }
        if start.elapsed() > Duration::from_secs(seconds) {
            bail!("child process exceeded {seconds}s deadline");
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    let mut out = Vec::new();
    let mut err = Vec::new();
    stdout.rewind()?;
    stderr.rewind()?;
    stdout.read_to_end(&mut out)?;
    stderr.read_to_end(&mut err)?;
    Ok(Output {
        code,
        stdout: out,
        stderr: err,
    })
}
pub fn checked(cmd: &mut Command, seconds: u64) -> Result<Vec<u8>> {
    let out = run(cmd, seconds, None, &Cancellation::default())?;
    if out.code != 0 {
        bail!(
            "command failed ({}): {}",
            out.code,
            String::from_utf8_lossy(&out.stderr)
        );
    }
    Ok(out.stdout)
}
pub fn text(cmd: &mut Command) -> Result<String> {
    Ok(String::from_utf8(checked(cmd, 120)?)?.trim().to_owned())
}
/// Stream long-running measurements to CI while retaining cancellation and descendant cleanup.
pub fn live(cmd: &mut Command, seconds: u64, cancel: &Cancellation) -> Result<()> {
    if cancel.cancelled() {
        bail!("operation interrupted");
    }
    cmd.stdin(Stdio::null())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let mut child = Child(cmd.spawn()?);
    let start = Instant::now();
    loop {
        if let Some(status) = child.0.try_wait()? {
            if !status.success() {
                bail!("child process failed: {status}");
            }
            return Ok(());
        }
        if cancel.cancelled() {
            bail!("operation interrupted");
        }
        if start.elapsed() > Duration::from_secs(seconds) {
            bail!("child process exceeded {seconds}s deadline");
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn cancellation_does_not_start_a_child_or_truncate_its_log() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("log");
        std::fs::write(&log, "retained").unwrap();
        let cancel = Cancellation::default();
        cancel.0.store(true, Ordering::Relaxed);
        assert!(run(
            Command::new("sh").args(["-c", "exit 0"]),
            10,
            Some(&log),
            &cancel
        )
        .is_err());
        assert_eq!(std::fs::read_to_string(log).unwrap(), "retained");
    }

    #[test]
    fn deadlines_kill_descendants_as_well_as_the_shell() {
        let dir = tempfile::tempdir().unwrap();
        let marker = dir.path().join("orphan");
        let pid_file = dir.path().join("pid");
        // The background child outlives the shell unless the whole process group is killed.
        let result = run(
            Command::new("sh")
                .args([
                    "-c",
                    "(sleep 2; touch \"$1\") & echo $! > \"$2\"; wait",
                    "test",
                ])
                .arg(&marker)
                .arg(&pid_file),
            1,
            None,
            &Cancellation::default(),
        );
        assert!(result.unwrap_err().to_string().contains("deadline"));
        assert!(
            pid_file.exists(),
            "the descendant must actually have started"
        );
        std::thread::sleep(Duration::from_millis(1300));
        assert!(!marker.exists(), "the descendant survived the deadline");
    }
}
