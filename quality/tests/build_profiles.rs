//! A warm Cargo target must still produce fresh build-script coverage.
use capntproto_test_support::verification as v;
use std::{fs, time::Duration};

#[test]
fn changing_profile_destination_reruns_cached_owned_build_scripts() {
    let directory = tempfile::tempdir().unwrap();
    let profiles = directory.path().join("profiles");
    fs::create_dir(&profiles).unwrap();
    for (step, destination, produces_profile) in [
        ("cold", "first", true),
        ("warm-new-profile", "second", true),
        ("warm-same-profile", "second", false),
    ] {
        let mut command = v::command("cargo");
        command
            .args(["check", "--locked", "--offline", "-p", "capntproto-rpc"])
            .env("CARGO_TARGET_DIR", directory.path().join("build"))
            .env("CARGO_INCREMENTAL", "0")
            .env("RUSTFLAGS", "-C instrument-coverage")
            .env_remove("CARGO_ENCODED_RUSTFLAGS")
            .env(
                "LLVM_PROFILE_FILE",
                profiles.join(format!("{destination}-%p-%m.profraw")),
            );
        v::run_with_timeout(
            &mut command,
            &directory.path().join(format!("{step}.log")),
            0,
            Duration::from_secs(120),
        )
        .unwrap();
        let files: Vec<_> = fs::read_dir(&profiles)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        assert_eq!(!files.is_empty(), produces_profile, "{step}: {files:?}");
        for file in files {
            assert!(file
                .file_name()
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with(destination));
            fs::remove_file(file).unwrap();
        }
    }
}
