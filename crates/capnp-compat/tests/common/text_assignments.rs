//! Ordered assignments and observable state after success or failure.
pub struct Case {
    pub schema: &'static str,
    pub seed: &'static str,
    pub input: &'static str,
    // Logical result; inactive union storage is checked separately against C++.
    #[allow(dead_code)] // Only the portable runner uses this expectation.
    pub expected: &'static str,
    pub error: bool,
    pub orphan: bool,
}

pub fn cases() -> Vec<Case> {
    let mut cases = vec![];
    let mut add = |schema, seed, input, expected, error, orphan| {
        cases.push(Case {
            schema,
            seed,
            input,
            expected,
            error,
            orphan,
        });
    };
    for (schema, input, expected) in [
        ("Record", "(i32 = 1, i32 = 2)", "(i32 = 2)"),
        (
            "Record",
            "(flag = true, flag = false, choice = second, choice = first)",
            "()",
        ),
        (
            "Record",
            r#"(text = "old", text = "new", data = 0x"ffff", data = 0x"00")"#,
            r#"(text = "new", data = 0x"00")"#,
        ),
        (
            "Record",
            "(nums = [1, 2], nums = [], nums = [3])",
            "(nums = [3])",
        ),
        (
            "Record",
            "(children = [(flag = true)], children = [(i8 = 7)], lists = [[1]], lists = [[]])",
            "(children = [(i8 = 7)], lists = [[]])",
        ),
        (
            "Record",
            r#"(child = (flag = true, text = "old"), child = (i8 = 7))"#,
            "(child = (i8 = 7))",
        ),
        (
            "Record",
            "(child = (i32 = 1, i32 = 2), children = [(i8 = 1, i8 = 2)])",
            "(child = (i32 = 2), children = [(i8 = 2)])",
        ),
        ("Record", r#"(none = void, some = "x")"#, r#"(some = "x")"#),
        ("Record", r#"(some = "hidden", none = void)"#, "()"),
        (
            "Record",
            r#"(some = "first", none = void, some = "last")"#,
            r#"(some = "last")"#,
        ),
        (
            "Wrapping",
            r#"(details = (value = 1, spare = "old"), details = 23)"#,
            "(details = (value = 23))",
        ),
        (
            "Wrapping",
            r#"(details = (value = 1), details = (spare = "new"))"#,
            r#"(details = (spare = "new"))"#,
        ),
        (
            "Wrapping",
            r#"(group = (spare = "old"), picked = false, group = true)"#,
            "(group = (value = true))",
        ),
        (
            "Wrapping",
            "(flag = false, flag = true, flags = [false, (value = false, value = true)])",
            "(flag = (value = true), flags = [(value = false), (value = true)])",
        ),
        (
            "AssignmentGroups",
            r#"(box = (pointer = "hidden", wide = 18446744073709551615, extra = 2, nested = (other = 18446744073709551615, note = "old")), box = ())"#,
            "()",
        ),
        (
            "AssignmentGroups",
            r#"(box = (nested = (other = 18446744073709551615), nested = (defaults = (value = 9))))"#,
            "(box = (nested = (defaults = (value = 9))))",
        ),
    ] {
        add(schema, "()", input, expected, false, false);
    }
    for (schema, seed, input, expected, error) in [
        (
            "Record",
            r#"(i32 = 7, text = "keep")"#,
            "(i32 = 1, i32 = 2)",
            r#"(i32 = 2, text = "keep")"#,
            false,
        ),
        (
            "Record",
            r#"(child = (flag = true, text = "old"), nums = [1, 2])"#,
            "(child = (), nums = [])",
            "(child = (), nums = [])",
            false,
        ),
        (
            "Wrapping",
            r#"(details = (value = 1, spare = "old"), group = (spare = "old"))"#,
            "(details = (), group = true)",
            "(group = (value = true))",
            false,
        ),
        (
            "AssignmentGroups",
            r#"(marker = 17, box = (pointer = "hidden", nested = (other = 18446744073709551615, note = "old")), picked = (value = 9, note = "old"))"#,
            "(box = (), picked = ())",
            "(marker = 17, picked = ())",
            false,
        ),
        (
            "Record",
            "(i8 = 7)",
            "(i32 = 12, i8 = 128, i32 = 13)",
            "(i8 = 7, i32 = 12)",
            true,
        ),
        ("Record", "(i8 = 7)", "(i8 = 128, i8 = 1)", "(i8 = 7)", true),
        (
            "Record",
            r#"(some = "keep")"#,
            "(none = 1, some = \"later\")",
            r#"(some = "keep")"#,
            true,
        ),
        ("Record", "()", "(some = 123)", "()", true),
        (
            "Record",
            r#"(child = (text = "keep"))"#,
            "(i32 = 12, child = (flag = true, i8 = 128))",
            r#"(i32 = 12, child = (text = "keep"))"#,
            true,
        ),
        (
            "Record",
            "(nums = [7], children = [(i8 = 3)])",
            "(nums = [1, true], children = [])",
            "(nums = [7], children = [(i8 = 3)])",
            true,
        ),
        (
            "Record",
            "(children = [(i8 = 3)])",
            "(children = [(i8 = 4), (i8 = 128)])",
            "(children = [(i8 = 3)])",
            true,
        ),
        (
            "Record",
            "(i32 = 7)",
            "(i32 = 12, missing = 1, i32 = 13)",
            "(i32 = 12)",
            true,
        ),
        (
            "Record",
            "(i32 = 7)",
            "(i32 = 12, i8 = )",
            "(i32 = 7)",
            true,
        ),
        (
            "Wrapping",
            r#"(details = (value = 1, spare = "old"))"#,
            "(details = (value = 23, spare = 123))",
            "(details = (value = 23))",
            true,
        ),
        (
            "Wrapping",
            r#"(details = (value = 1, spare = "old"))"#,
            "(details = false)",
            "()",
            true,
        ),
        (
            "Wrapping",
            "(picked = false)",
            "(group = (value = true, spare = 123))",
            "(group = (value = true))",
            true,
        ),
        (
            "Wrapping",
            "(picked = false)",
            "(group = 123)",
            "(group = ())",
            true,
        ),
        (
            "AssignmentGroups",
            r#"(marker = 17, box = (pointer = "hidden", nested = (other = 18446744073709551615, note = "old")))"#,
            "(box = (extra = 23, nested = (defaults = (value = 9), note = 123)))",
            "(marker = 17, box = (extra = 23, nested = (defaults = (value = 9))))",
            true,
        ),
    ] {
        add(schema, seed, input, expected, error, false);
    }
    for (seed, input, expected, error) in [
        (
            "(spare = 99)",
            "(value = 1, other = \"old\", value = 23)",
            "(value = 23)",
            false,
        ),
        (
            "(value = 9, spare = 99)",
            "(other = \"partial\", spare = true)",
            "(value = 9, spare = 99)",
            true,
        ),
    ] {
        add("UnionBox", seed, input, expected, error, true);
    }
    cases
}
