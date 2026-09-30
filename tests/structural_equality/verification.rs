use super::super::*;
use reproto_test_support::verification::{command, cpp, exploration, root, run};

fn result_code(result: capnp::Result<Equality>) -> u64 {
    match result {
        Ok(Equality::Equal) => 1,
        Ok(Equality::NotEqual) => 2,
        Ok(Equality::UnknownContainsCapabilities) => 3,
        Err(_) => 4,
    }
}

#[derive(Default)]
struct Oracle {
    inputs: String,
    expected: Vec<u64>,
}
impl Oracle {
    fn observe(
        &mut self,
        left: &[Vec<u64>],
        right: &[Vec<u64>],
        options: message::ReaderOptions,
        mode: u64,
    ) -> u64 {
        let result = result_code(compare_segments(left, right, options, mode));
        self.inputs += &format!(
            "{mode} {} {} ",
            options.nesting_limit,
            options.traversal_limit_in_words.unwrap() as u64
        );
        for message in [left, right] {
            self.inputs += &format!("{} ", message.len());
            for segment in message {
                self.inputs += &format!("{} ", segment.len());
                for word in segment {
                    self.inputs += &format!("{word} ");
                }
            }
        }
        self.inputs.push('\n');
        self.expected.push(result);
        result
    }
    fn check(self) {
        let build = cpp::build(&["capnp"]).unwrap();
        let logs = root().join("target/verification/structural-equality-cpp");
        let tmp = tempfile::tempdir().unwrap();
        let exe = tmp.path().join("structural-equality");
        run(
            command("g++")
                .args([
                    "-std=c++23",
                    "-Ivendor/capnproto/c++/src",
                    "tests/cpp/structural-equality.c++",
                ])
                .arg(build.join("c++/src/capnp/libcapnp.a"))
                .arg(build.join("c++/src/kj/libkj.a"))
                .args(["-pthread", "-o"])
                .arg(&exe),
            &logs.join("compile.log"),
            0,
        )
        .unwrap();
        let input = logs.join("cases.txt");
        std::fs::write(&input, &self.inputs).unwrap();
        let output = run(command(exe).arg(input), &logs.join("reference.log"), 0).unwrap();
        let actual: Vec<u64> = output.lines().map(|line| line.parse().unwrap()).collect();
        assert_eq!(actual.len(), self.expected.len());
        for (index, ((a, b), case)) in actual
            .into_iter()
            .zip(self.expected)
            .zip(self.inputs.lines())
            .enumerate()
        {
            assert_eq!(a, b, "C++ observation {index}: {case}");
        }
    }
}

#[test]
fn tlc_structural_comparison_traces_match_rust_and_cpp() {
    const MODEL: &str = "verification/StructuralEquality.tla";
    const CONFIG: &str = include_str!("../../verification/StructuralEquality.cfg");
    let paths = exploration::traces(MODEL, "structural-equality", CONFIG).unwrap();
    let traces = paths.len();
    let mut oracle = Oracle::default();
    for path in paths {
        let (mut left, mut right, mut result) = (0, 0, 0);
        for state in &path {
            match state["event"] {
                event @ 1..=36 => left = event - 1,
                event @ 101..=136 => right = event - 101,
                event @ 201..=202 => {
                    let (a, b) = if event == 201 {
                        (left, right)
                    } else {
                        (right, left)
                    };
                    result =
                        oracle.observe(&[shape(a)], &[shape(b)], message::ReaderOptions::new(), 0);
                }
                event => panic!("unexpected event {event}"),
            }
            assert_eq!(
                [left, right, result],
                [state["left"], state["right"], state["result"]],
                "{path:?}"
            );
        }
    }
    eprintln!(
        "{} Rust/C++ structural comparisons across {traces} edge-prefix scenarios",
        oracle.expected.len()
    );
    oracle.check();
    exploration::controls(
        MODEL,
        "structural-equality",
        CONFIG,
        &[
            ("comparePadding", "StructuralContract"),
            ("compareNullFields", "StructuralContract"),
            ("ignoreEncoding", "StructuralContract"),
            ("ignoreLength", "StructuralContract"),
            ("bitPadding", "StructuralContract"),
            ("capIdentity", "StructuralContract"),
            ("stopAtUnknown", "StructuralContract"),
            ("forgetUnknown", "StructuralContract"),
        ],
        None,
    )
    .unwrap();
}

#[test]
fn malformed_far_and_bounded_readers_match_cpp() {
    let mut oracle = Oracle::default();
    let options = message::ReaderOptions::new();
    // Direct, single-far, and double-far encodings of the same struct.
    let direct = vec![shape(3)];
    let single = vec![vec![2 | (1 << 32)], shape(3)];
    let double = vec![vec![6 | (1 << 32)], vec![2 | (2 << 32), 1 << 32], vec![42]];
    for a in [&direct, &single, &double] {
        for b in [&direct, &single, &double] {
            assert_eq!(oracle.observe(a, b, options, 0), 1);
        }
    }
    for malformed in [
        vec![vec![7]],
        vec![vec![1 << 32]],
        vec![vec![2 | (9 << 32)]],
        vec![vec![6 | (1 << 32)], vec![2 | (9 << 32), 1 << 32]],
        vec![primitive(7, 1, &[4 | (2 << 32), 0])],
        vec![structure(&[], &[cap(0), vec![7]])],
        vec![vec![1 << 48, (1 << 48) | 0xffff_fffc]],
    ] {
        assert_eq!(oracle.observe(&malformed, &malformed, options, 0), 4);
    }
    for depth in 0..=4 {
        let mut bounded = options;
        bounded.nesting_limit = depth;
        for i in [0, 1, 3, 24, 26, 29, 32, 34, 35] {
            oracle.observe(&[shape(i)], &[shape(i)], bounded, 0);
        }
    }
    // Resource accounting may differ across implementations: exercise cases
    // with ample budget or guaranteed exhaustion, not the exact word boundary.
    let mut bounded = options;
    bounded.traversal_limit_in_words = Some(0);
    assert_eq!(oracle.observe(&direct, &direct, bounded, 0), 4);
    for (a, b) in [(0, 1), (1, 2), (3, 4), (6, 9), (10, 11)] {
        oracle.observe(&[shape(a)], &[shape(b)], options, 1);
    }
    for (a, b) in [(26, 27), (26, 28), (29, 30), (29, 31)] {
        oracle.observe(&[shape(a)], &[shape(b)], options, 2);
    }
    // Float comparison is bitwise, and list padding stops at the declared
    // element count. Exercise every physical primitive width and byte boundary.
    for (encoding, width) in [(1, 1u64), (2, 8), (3, 16), (4, 32), (5, 64)] {
        for count in [0u64, 1, 7, 8, 9, 16, 17, 64, 65] {
            let a = primitive(
                encoding,
                count,
                &vec![0; (count * width).div_ceil(64) as usize],
            );
            assert_eq!(
                oracle.observe(
                    std::slice::from_ref(&a),
                    std::slice::from_ref(&a),
                    options,
                    0
                ),
                1
            );
            if count > 0 {
                let bit = (count - 1) * width;
                let mut b = a.clone();
                b[1 + (bit / 64) as usize] |= 1 << (bit % 64);
                assert_eq!(
                    oracle.observe(std::slice::from_ref(&a), &[b], options, 0),
                    2
                );
                if count * width % 64 != 0 {
                    let mut b = a.clone();
                    *b.last_mut().unwrap() |= u64::MAX << (count * width % 64);
                    assert_eq!(oracle.observe(&[a], &[b], options, 0), 1);
                }
            }
        }
    }
    for (a, b, result) in [
        (0.0f64.to_bits(), (-0.0f64).to_bits(), 2),
        (f64::NAN.to_bits(), f64::NAN.to_bits(), 1),
        (f64::NAN.to_bits(), f64::NAN.to_bits() ^ 1, 2),
    ] {
        assert_eq!(
            oracle.observe(
                &[primitive(5, 1, &[a])],
                &[primitive(5, 1, &[b])],
                options,
                0
            ),
            result
        );
    }
    eprintln!(
        "{} supplementary Rust/C++ structural comparisons",
        oracle.expected.len()
    );
    oracle.check();
}
