// Inputs shared by portable tests and the pinned C++ codec oracle.
pub fn accepted() -> Vec<String> {
    let mut cases = vec![
        "(u8 = -0, u16 = -00, u32 = -0x0, u64 = - 0, f32 = -0, f64 = -0)".into(),
        "(f32 = -0.0, f64 = -0e0)".into(),
        "(f32 = 1e9999, f64 = -1e9999)".into(),
        "(f32 = 1e-9999, f64 = -1e-9999)".into(),
        "(f32 = 01e1, f64 = 07.85)".into(),
        "(i64 = -9223372036854775808, u64 = 18446744073709551615)".into(),
        "(f32 = 18446744073709551615, f64 = -9223372036854775808)".into(),
        "(data = 0x\"\x0b00\x0bff\x0b\")".into(),
    ];
    // One integer either side of Float32 half-way points. Rounding via f64
    // loses the low bits, so its answer differs from a direct integer cast.
    for exponent in [54, 62] {
        for offset in [-1_i64, 0, 1] {
            let magnitude = ((1_i64 << exponent) + (1_i64 << (exponent - 24)) + offset) as u64;
            for number in [
                format!("{magnitude}"),
                format!("0x{magnitude:x}"),
                format!("0{magnitude:o}"),
            ] {
                for sign in ["", "-"] {
                    cases.push(format!("(f32 = {sign}{number}, f64 = {sign}{number})"));
                }
            }
        }
    }
    cases
}

pub fn rejected() -> Vec<String> {
    let mut cases = vec![];
    for field in ["text", "choice", "data", "flag", "none", "child"] {
        for value in ["0", "1", "-1", "1.0"] {
            cases.push(format!("({field} = {value})"));
        }
    }
    for field in ["i64", "u64"] {
        for value in ["1.0", "1e0", "-0.0", "1e9999"] {
            cases.push(format!("({field} = {value})"));
        }
    }
    for number in [
        "0Xff",
        "0b10",
        "0o10",
        "08e1",
        "1_0",
        "-9223372036854775809",
        "-0x8000000000000001",
    ] {
        cases.push(format!("(f64 = {number})"));
    }
    cases.extend(
        [
            "(u8 = 256)",
            "(i8 = -129)",
            "(u64 = -1)",
            "(i64 = 9223372036854775808)",
        ]
        .map(str::to_owned),
    );
    cases
}
