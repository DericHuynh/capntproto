// Shared by the portable tests and real C++ TextCodec comparisons.
pub struct Accepted {
    pub schema: &'static str,
    pub input: &'static str,
    #[allow(dead_code)] // Explicit equivalence is checked by the portable runner.
    pub explicit: &'static str,
    pub orphan: bool,
}

pub fn accepted() -> Vec<Accepted> {
    let mut cases = vec![];
    for (input, explicit) in [
        ("(flag = false)", "(flag = (value = false))"),
        (
            "(flag = true, choice = (value = true))",
            "(flag = (value = true), choice = (value = true))",
        ),
        (
            "(number = 4611686293305294849)",
            "(number = (value = 4611686293305294849))",
        ),
        ("(number = -0.0)", "(number = (value = -0.0))"),
        ("(number = nan)", "(number = (value = nan))"),
        ("(number = -inf)", "(number = (value = -inf))"),
        ("(number = 1e9999)", "(number = (value = 1e9999))"),
        ("(integer = -128)", "(integer = (value = -128))"),
        ("(emptyValue = void)", "(emptyValue = (value = void))"),
        (
            r#"(text = "a\x00\a\b\f\v\'\"\\\xc3\xa9🦀")"#,
            r#"(text = (value = "a\x00\a\b\f\v\'\"\\\xc3\xa9🦀"))"#,
        ),
        (r#"(text = "")"#, r#"(text = (value = ""))"#),
        ("(unionBox = 42)", "(unionBox = (value = 42))"),
        ("(unionBox = -0)", "(unionBox = (value = 0))"),
        (
            "(flags = [false, true, (value = false, spare = \"explicit\")])",
            "(flags = [(value = false), (value = true), (value = false, spare = \"explicit\")])",
        ),
        (
            r#"(texts = ["a", "", (value = "b")])"#,
            r#"(texts = [(value = "a"), (value = ""), (value = "b")])"#,
        ),
        (
            "(numbers = [0x20, -0.0, inf])",
            "(numbers = [(value = 0x20), (value = -0.0), (value = inf)])",
        ),
        ("(details = 23)", "(details = (value = 23))"),
        ("(picked = false)", "(picked = (value = false))"),
        ("(group = true)", "(group = (value = true))"),
        (
            r#"(nested = (value = "nested"))"#,
            r#"(nested = (value = (value = "nested")))"#,
        ),
    ] {
        cases.push(Accepted {
            schema: "Wrapping",
            input,
            explicit,
            orphan: false,
        });
    }
    for (schema, input, explicit) in [
        ("BoolBox", "false", "(value = false)"),
        ("BoolBox", "true", "(value = true)"),
        (
            "NumberBox",
            "-4611686293305294849",
            "(value = -4611686293305294849)",
        ),
        ("VoidBox", "void", "(value = void)"),
        ("UnionBox", "0xff", "(value = 0xff)"),
    ] {
        cases.push(Accepted {
            schema,
            input,
            explicit,
            orphan: true,
        });
    }
    cases
}

pub struct Rejected {
    pub schema: &'static str,
    pub input: &'static str,
    pub orphan: bool,
}

pub fn rejected() -> Vec<Rejected> {
    let mut cases = vec![];
    for input in [
        "(integer = 128)",
        "(integer = 1.0)",
        "(flag = 1)",
        "(number = true)",
        "(emptyValue = false)",
        "(text = 123)",
        "(text = 0x\"00ff\")",
        "(data = \"bytes\")",
        "(data = 0x\"00ff\")",
        "(choice = first)",
        "(choice = true)",
        "(choice = 0)",
        "(nested = \"two levels\")",
        "(list = [1,2])",
        "(empty = true)",
        "(groupBox = true)",
        "(details = true)",
        "(group = 1)",
        "(flags = [true, 1])",
    ] {
        cases.push(Rejected {
            schema: "Wrapping",
            input,
            orphan: false,
        });
    }
    for (schema, input, orphan) in [
        ("BoolBox", "true", false), // Existing-root overload requires a tuple.
        ("EmptyBox", "void", true),
        ("EnumBox", "first", true),
        ("GroupBox", "true", true),
    ] {
        cases.push(Rejected {
            schema,
            input,
            orphan,
        });
    }
    cases
}
