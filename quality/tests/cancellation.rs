//! Exercise cancellation in subprocesses so signal state cannot poison other tests.
#![cfg(target_os = "linux")]
use capntproto_test_support::verification as v;
use std::{
    fs,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

#[test]
fn cancellation_worker() {
    let Ok(directory) = std::env::var("CAPNTPROTO_CANCEL_TEST") else {
        return;
    };
    let directory = std::path::Path::new(&directory);
    let mut command = if std::env::var_os("CAPNTPROTO_CANCEL_NESTED").is_none() {
        let mut cmd = Command::new(std::env::current_exe().unwrap());
        cmd.args(["--exact", "cancellation_worker"])
            .env("CAPNTPROTO_CANCEL_NESTED", "1");
        cmd
    } else {
        let mut cmd = Command::new("sh");
        cmd.args([
            "-c",
            "trap '' TERM; echo $$ > \"$1\"; exec sleep 60",
            "worker",
        ])
        .arg(directory.join("leaf.pid"));
        cmd
    };
    let log = if std::env::var_os("CAPNTPROTO_CANCEL_NESTED").is_some() {
        "leaf.log"
    } else {
        "nested.log"
    };
    assert!(v::run_with_timeout(
        &mut command,
        &directory.join(log),
        0,
        Duration::from_secs(10)
    )
    .is_err());
}

#[test]
fn termination_stops_nested_groups_and_term_resistant_children() {
    let dir = tempfile::tempdir().unwrap();
    let mut worker = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "cancellation_worker"])
        .env("CAPNTPROTO_CANCEL_TEST", dir.path())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let start = Instant::now();
    let leaf = loop {
        if let Ok(pid) = fs::read_to_string(dir.path().join("leaf.pid")) {
            break pid.trim().parse::<u32>().unwrap();
        }
        if start.elapsed() > Duration::from_secs(5) {
            let _ = worker.kill();
            let _ = worker.wait();
            panic!("nested worker did not become ready");
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    assert!(Command::new("kill")
        .args(["-TERM", &worker.id().to_string()])
        .status()
        .unwrap()
        .success());
    loop {
        if let Some(status) = worker.try_wait().unwrap() {
            assert!(status.success(), "worker did not handle cancellation");
            break;
        }
        if start.elapsed() > Duration::from_secs(5) {
            let _ = worker.kill();
            let _ = worker.wait();
            panic!("cancellation did not complete promptly");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    if let Ok(stat) = fs::read_to_string(format!("/proc/{leaf}/stat")) {
        assert_eq!(
            stat.rsplit_once(") ").unwrap().1.split_whitespace().next(),
            Some("Z"),
            "terminated child remains running"
        );
    }
}
