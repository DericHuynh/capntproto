#![cfg(all(feature = "services", feature = "tls"))]
//! Real server/client processes, including shared state and capability pipelines.
use capntproto_test_support::verification::{command, root, run};
use rcgen::{
    BasicConstraints, CertificateParams, CertifiedIssuer, ExtendedKeyUsagePurpose, IsCa, KeyPair,
    KeyUsagePurpose,
};
use std::{
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::mpsc,
    time::Duration,
};

struct Server(Child);
impl Drop for Server {
    fn drop(&mut self) {
        // Let the server complete its Ctrl-C path and flush instrumented
        // profiles. Keep a bounded kill fallback for a failed server.
        #[cfg(unix)]
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = Command::new("kill")
                .arg("-INT")
                .arg(self.0.id().to_string())
                .status();
            for _ in 0..100 {
                if self.0.try_wait().ok().flatten().is_some() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn target() -> PathBuf {
    std::env::var_os("CAPNTPROTO_FULL_COVERAGE_BUILD")
        .map(PathBuf::from)
        .unwrap_or_else(|| root().join("target"))
        .join("service-examples")
}

fn executable(name: &str) -> PathBuf {
    target()
        .join("debug")
        .join(format!("{name}{}", std::env::consts::EXE_SUFFIX))
}

fn start(args: &[String]) -> (Server, String) {
    let mut child = Server(
        Command::new(executable("server"))
            .args(["--listen", "127.0.0.1:0"])
            .args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    let stdout = child.0.stdout.take().unwrap();
    let (send, receive) = mpsc::channel();
    std::thread::spawn(move || {
        let mut line = String::new();
        let result = BufReader::new(stdout).read_line(&mut line).map(|_| line);
        let _ = send.send(result);
    });
    let line = receive
        .recv_timeout(Duration::from_secs(20))
        .expect("service did not become ready")
        .unwrap();
    let address = line
        .split_whitespace()
        .nth(2)
        .expect("service readiness address")
        .split_once("://")
        .unwrap()
        .1
        .to_owned();
    (child, address)
}

fn client(binary: &str, args: &[String], success: bool) -> String {
    let output = Command::new(executable(binary))
        .args(args)
        .output()
        .unwrap();
    assert_eq!(
        output.status.success(),
        success,
        "{binary} {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn append(base: &[String], extra: &[&str]) -> Vec<String> {
    base.iter()
        .cloned()
        .chain(extra.iter().map(|s| s.to_string()))
        .collect()
}

fn certificates(directory: &Path) {
    let mut params = CertificateParams::new(Vec::<String>::new()).unwrap();
    params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    params.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    let issuer = CertifiedIssuer::self_signed(params, KeyPair::generate().unwrap()).unwrap();
    let key = KeyPair::generate().unwrap();
    let mut params = CertificateParams::new(vec!["localhost".into()]).unwrap();
    params.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
    let cert = params.signed_by(&key, &issuer).unwrap();
    std::fs::write(directory.join("ca.pem"), issuer.pem()).unwrap();
    std::fs::write(directory.join("server.pem"), cert.pem()).unwrap();
    std::fs::write(directory.join("server-key.pem"), key.serialize_pem()).unwrap();
}

fn example_cargo() -> (Command, bool) {
    let mut build = command("cargo");
    let instrumented = if let (Ok(profiles), Ok(flags)) = (
        std::env::var("CAPNTPROTO_FULL_COVERAGE_PROFILES"),
        std::env::var("CAPNTPROTO_FULL_COVERAGE_FLAGS"),
    ) {
        // Match the parent lane's LLVM version and include these real service
        // binaries in its existing recursive object/profile collection.
        build
            .arg("+nightly-2026-03-05")
            .env("RUSTFLAGS", &flags)
            .env("RUSTDOCFLAGS", &flags)
            .env(
                "LLVM_PROFILE_FILE",
                PathBuf::from(profiles).join("%p-%m.profraw"),
            );
        true
    } else {
        false
    };
    (build, instrumented)
}

#[test]
fn independent_clients_share_a_real_service_and_pipeline_deep_access() {
    let directory = tempfile::tempdir().unwrap();
    let (mut build, instrumented) = example_cargo();
    build
        .args([
            "build",
            "--locked",
            "--manifest-path",
            "examples/kv/Cargo.toml",
            "--bins",
            "--target-dir",
        ])
        .arg(target());
    if instrumented {
        build.arg("--ignore-rust-version");
    }
    if cfg!(feature = "quic") {
        build.args(["--features", "quic"]);
    }
    run(&mut build, &directory.path().join("build.log"), 0).unwrap();
    if cfg!(feature = "quic") {
        let (mut test, instrumented) = example_cargo();
        test.args([
            "test",
            "--locked",
            "--manifest-path",
            "examples/kv/Cargo.toml",
            "--lib",
            "--features",
            "quic",
            "--target-dir",
        ])
        .arg(target());
        if instrumented {
            test.arg("--ignore-rust-version");
        }
        run(&mut test, &directory.path().join("address-fallback.log"), 0).unwrap();
    }
    certificates(directory.path());
    let path = |name| directory.path().join(name).display().to_string();
    let mut modes = vec![("tcp", false), ("tcp", true)];
    if cfg!(feature = "quic") {
        modes.push(("quic", true));
    }
    for (transport, encrypted) in modes {
        let mut server_args = vec!["--transport".into(), transport.into()];
        if encrypted {
            server_args.extend([
                "--cert".into(),
                path("server.pem"),
                "--key".into(),
                path("server-key.pem"),
            ]);
        }
        let (_server, address) = start(&server_args);
        let mut base = vec![
            "--server".into(),
            address,
            "--transport".into(),
            transport.into(),
        ];
        if encrypted {
            base.extend(["--ca".into(), path("ca.pem")]);
        }
        assert_eq!(
            client(
                "client",
                &append(&base, &["put", "greeting", "hello"]),
                true
            )
            .trim(),
            "stored greeting"
        );
        // A new process and connection must see the first process's write.
        assert_eq!(
            client("client", &append(&base, &["get", "greeting"]), true).trim(),
            "hello"
        );
        // A hostname may resolve to IPv6 before IPv4 even though this service
        // deliberately listens only on IPv4. Every transport must try fallback.
        let mut named = base.clone();
        let address: std::net::SocketAddr = named[1].parse().unwrap();
        named[1] = format!("localhost:{}", address.port());
        assert_eq!(
            client("client", &append(&named, &["get", "greeting"]), true).trim(),
            "hello"
        );
        client(
            "client",
            &append(&base, &["--path", "users/alice", "put", "name", "Alice"]),
            true,
        );
        assert_eq!(
            client(
                "client",
                &append(&base, &["--path", "users/alice", "get", "name"]),
                true
            )
            .trim(),
            "Alice"
        );
        client("client", &append(&base, &["get", "name"]), false);
        assert_eq!(
            client("client", &append(&base, &["delete", "greeting"]), true).trim(),
            "deleted=true"
        );
        client("client", &append(&base, &["get", "greeting"]), false);
        client("deep-client", &base, true);
        let deep_path = (0..32)
            .map(|n| format!("level-{n}"))
            .collect::<Vec<_>>()
            .join("/");
        assert_eq!(
            client(
                "client",
                &append(&base, &["--path", &deep_path, "get", "client-17"]),
                true
            )
            .trim(),
            "value-17"
        );
        if encrypted {
            // Certificate verification must not fall back to plaintext.
            client(
                "client",
                &append(
                    &base,
                    &["--server-name", "wrong.example", "get", "greeting"],
                ),
                false,
            );
        }
    }
}

#[test]
fn pem_configuration_roundtrips_and_preserves_actionable_errors() {
    use capntproto::rpc::{security, tcp::tls};
    use std::io::ErrorKind;
    let directory = tempfile::tempdir().unwrap();
    certificates(directory.path());
    let path = |name| directory.path().join(name);
    let identity =
        security::Identity::from_pem_files(path("server.pem"), path("server-key.pem")).unwrap();
    assert_eq!(identity.certificates.len(), 1);
    tls::server_config(identity, None).unwrap();
    assert_eq!(
        security::roots_from_pem_file(path("ca.pem")).unwrap().len(),
        1
    );
    assert_eq!(
        security::certificates_from_pem_file(path("missing.pem"))
            .unwrap_err()
            .kind(),
        ErrorKind::NotFound
    );
    std::fs::write(path("empty.pem"), "").unwrap();
    assert_eq!(
        security::certificates_from_pem_file(path("empty.pem"))
            .unwrap_err()
            .kind(),
        ErrorKind::InvalidData
    );
    assert!(security::Identity::from_pem_files(path("server.pem"), path("empty.pem")).is_err());
    std::fs::write(
        path("invalid.pem"),
        "-----BEGIN CERTIFICATE-----\nAQID\n-----END CERTIFICATE-----\n",
    )
    .unwrap();
    assert_eq!(
        security::roots_from_pem_file(path("invalid.pem"))
            .unwrap_err()
            .kind(),
        ErrorKind::InvalidInput
    );
}
