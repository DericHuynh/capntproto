// Shared by the portable compiler tests and the pinned C++ differential test.
// Each source is independent: no system imports or filesystem assumptions.
pub struct Case {
    pub name: String,
    pub body: String,
    pub accepted: bool,
}
impl Case {
    pub fn source(&self) -> String {
        format!("@0xabcdefabcdefabcd;\n{}\n", self.body)
    }
}

pub fn lexical_cases() -> Vec<Case> {
    let mut cases = Vec::new();
    // All ASCII controls plus space and DEL. Raw LF terminates a quoted string;
    // other controls remain bytes inside it. Outside strings, only KJ's six
    // whitespace bytes separate tokens and binary byte pairs.
    for byte in (0..=32).chain([127]) {
        let ch = char::from(byte);
        let whitespace = matches!(byte, b' ' | b'\n' | b'\r' | b'\t' | 11 | 12);
        for (context, body, accepted) in [
            (
                "tokens",
                format!("struct{ch}S {{ value @0 :UInt32; }}"),
                whitespace,
            ),
            (
                "binary",
                format!("const data :Data = 0x\"{ch}00{ch}ff{ch}\";"),
                whitespace,
            ),
            (
                "quoted",
                format!("const text :Text = \"a{ch}b\";"),
                byte != b'\n',
            ),
        ] {
            cases.push(Case {
                name: format!("{context}-{byte}"),
                body,
                accepted,
            });
        }
    }
    for (name, spacing) in [
        ("nbsp", '\u{a0}'),
        ("bom", '\u{feff}'),
        ("em-space", '\u{2003}'),
    ] {
        cases.push(Case {
            name: format!("tokens-{name}"),
            body: format!("struct{spacing}S {{}}"),
            accepted: spacing == '\u{feff}',
        });
        cases.push(Case {
            name: format!("binary-{name}"),
            body: format!("const data :Data = 0x\"00{spacing}ff\";"),
            accepted: false,
        });
    }
    cases
}
pub fn cases() -> Vec<Case> {
    let mut cases = Vec::new();
    let mut push = |name: String, body: String, accepted| {
        cases.push(Case {
            name,
            body,
            accepted,
        })
    };
    for (name, body, accepted) in [
        ("empty-statement", ";", false),
        ("trailing-struct-semicolon", "struct S {};", false),
        ("empty-member", "struct S { ; }", false),
        ("struct-semicolon", "struct S;", false),
        ("enum-semicolon", "enum E;", false),
        ("interface-semicolon", "interface I;", false),
        (
            "duplicate-parameters",
            "struct S(T,T) { value @0 :T; }",
            true,
        ),
        ("lowercase-parameter", "struct S(t) { value @0 :t; }", true),
        (
            "underscore-parameter",
            "struct S(Bad_Name) { value @0 :Bad_Name; }",
            true,
        ),
        (
            "nested-type-shadows-parameter",
            "struct S(T) { struct T {} value @0 :T; }",
            true,
        ),
        (
            "alias-shadows-parameter",
            "struct S(T) { using T = Text; value @0 :T; }",
            true,
        ),
        (
            "duplicate-method-parameters",
            "interface I { f @0 [T,T] (x :T); }",
            true,
        ),
        (
            "lowercase-method-parameter",
            "interface I { f @0 [t] (x :t); }",
            true,
        ),
        ("no-annotation-targets", "annotation note() :Void;", true),
        (
            "duplicate-annotation-targets",
            "annotation note(struct,struct) :Void;",
            false,
        ),
        (
            "mixed-annotation-targets",
            "annotation note(*,struct) :Void;",
            false,
        ),
        (
            "trailing-value-comma",
            "struct S { f @0 :UInt32 = (1,); }",
            true,
        ),
        (
            "nested-value-parentheses",
            "struct S { f @0 :UInt32 = ((1)); }",
            true,
        ),
        (
            "absolute-value-comma",
            "const n :UInt32 = 1; struct S { f @0 :UInt32 = (.n,); }",
            true,
        ),
        (
            "empty-list-element",
            "struct S { f @0 :List(UInt32) = [1,,2]; }",
            false,
        ),
        (
            "trailing-list-comma",
            "struct S { f @0 :List(UInt32) = [1,]; }",
            true,
        ),
        (
            "empty-generic-arguments",
            "struct S(T) {} struct X { f @0 :S(); }",
            false,
        ),
        (
            "named-generic-argument",
            "struct S(T) {} struct X { f @0 :S(T = Text); }",
            false,
        ),
        ("empty-extends", "interface I extends () {}", true),
        (
            "duplicate-extends",
            "interface A {} interface I extends (A,A) {}",
            true,
        ),
        (
            "empty-method-parameters",
            "interface I { f @0 () -> (); }",
            true,
        ),
        (
            "void-method-parameters",
            "interface I { f @0 Void; }",
            false,
        ),
        (
            "void-method-results",
            "interface I { f @0 () -> Void; }",
            false,
        ),
        (
            "trailing-method-comma",
            "interface I { f @0 (x :UInt32 = 1,); }",
            true,
        ),
        (
            "union-field-without-ordinal",
            "struct S { union :UInt32; }",
            false,
        ),
        (
            "contextual-field-names",
            "struct S { group @0 :Text; using @1 :Text; }",
            true,
        ),
        (
            "alias-in-union",
            "struct S { union { using T = UInt32; a @0 :T; b @1 :Text; } }",
            false,
        ),
        (
            "type-in-union",
            "struct S { union { struct T {} a @0 :T; b @1 :Text; } }",
            false,
        ),
        (
            "alias-in-group",
            "struct S { g :group { using T = UInt32; a @0 :T; } }",
            false,
        ),
        (
            "type-in-group",
            "struct S { g :group { struct T {} a @0 :T; } }",
            false,
        ),
        ("alias-in-enum", "enum E { using X = UInt32; a @0; }", false),
        ("type-in-enum", "enum E { struct S {} a @0; }", false),
        ("positive-sign", "const n :Float64 = +1;", false),
        ("leading-decimal-point", "const n :Float64 = .5;", false),
        ("trailing-decimal-point", "const n :Float64 = 1.;", true),
        ("negative-nan", "const n :Float64 = -nan;", false),
        ("hex-float", "const n :Float64 = 0x1p0;", false),
        ("positive-exponent", "const n :Float64 = 1e+2;", true),
        ("spaced-negative", "const n :Float64 = - 1;", true),
        ("concatenated-text", "const n :Text = \"a\" \"b\";", true),
        (
            "concatenated-data",
            "const n :Data = 0x\"00\" 0x\"ff\";",
            false,
        ),
        (
            "uppercase-alias",
            "using X = UInt32; struct S { f @0 :X; }",
            true,
        ),
        (
            "lowercase-type-alias",
            "using x = UInt32; struct S { f @0 :x; }",
            false,
        ),
        (
            "constant-alias",
            "const n :UInt32 = 1; using m = .n; struct S { f @0 :UInt32 = .m; }",
            true,
        ),
        (
            "duplicate-field-constant",
            "struct S { const x :UInt32 = 1; x @0 :UInt32; }",
            false,
        ),
        (
            "duplicate-method-constant",
            "interface I { const f :UInt32 = 1; f @0 (); }",
            false,
        ),
        (
            "duplicate-field-group",
            "struct S { a @0 :Text; a :group { b @1 :Bool; } }",
            false,
        ),
        (
            "annotation-trailing-comma",
            "annotation n(struct) :UInt32; struct S $n(1,) {}",
            true,
        ),
        (
            "empty-annotation-value",
            "annotation n(struct) :Void; struct S $n() {}",
            false,
        ),
        (
            "explicit-void-annotation",
            "annotation n(struct) :Void; struct S $n(void) {}",
            true,
        ),
        (
            "empty-void-default",
            "struct S { foo @0 :Void = (); }",
            false,
        ),
        ("empty-void-constant", "const x :Void = ();", false),
    ] {
        push(name.into(), body.into(), accepted);
    }
    // Keywords and built-ins are contextual, including when used as parameters.
    for name in [
        "import",
        "embed",
        "stream",
        "List",
        "AnyPointer",
        "AnyStruct",
        "AnyList",
        "Capability",
        "Void",
        "Bool",
        "Text",
        "Data",
        "struct",
        "enum",
        "interface",
        "const",
        "annotation",
        "using",
        "union",
        "group",
        "extends",
        "true",
        "false",
        "inf",
        "nan",
    ] {
        // Bare `union` after a field's colon still selects named-union syntax.
        push(
            format!("struct-parameter-{name}"),
            format!("struct S({name}) {{ value @0 :{name}; }}"),
            name != "union",
        );
        push(
            format!("method-parameter-{name}"),
            format!("interface I {{ call @0 [{name}] (x :{name}) -> (y :{name}); }}"),
            true,
        );
    }
    for name in [
        "import", "embed", "stream", "union", "using", "group", "true", "false", "inf", "nan",
    ] {
        push(
            format!("constant-{name}"),
            format!("const {name} :UInt32 = 7; const x :UInt32 = .{name};"),
            true,
        );
        push(
            format!("field-{name}"),
            format!("struct S {{ {name} @0 :UInt32; }}"),
            true,
        );
    }
    for (index, (expression, accepted)) in [
        ("List()", false),
        ("List(Text,)", true),
        ("((Text),)", true),
        ("((Text),)(Text)", false),
        ("Text()", true),
        ("Void()", true),
        ("List(Void)", true),
        ("List(Capability)", true),
        ("List(AnyList)", true),
    ]
    .into_iter()
    .enumerate()
    {
        push(
            format!("type-expression-{index}"),
            format!("struct X {{ f @0 :{expression}; }}"),
            accepted,
        );
    }
    for (index, (number, accepted)) in [
        ("1.0", true),
        ("1.", true),
        ("1.e2", true),
        ("1.e+2", true),
        ("1.e-2", true),
        ("0Xff", false),
        ("0xFF", true),
        ("00.5", true),
        ("01e1", true),
        ("08e1", false),
        ("00e+1", true),
        ("1E2", true),
        ("1E+2", true),
        ("1E-2", true),
        ("- 0", true),
        ("- 0.0", true),
        ("1_0", false),
        ("0b10", false),
        ("0o10", false),
    ]
    .into_iter()
    .enumerate()
    {
        push(
            format!("number-form-{index}"),
            format!("const x :Float64 = {number};"),
            accepted,
        );
    }
    // C++ tries octal integers before floats. An 8/9 in a leading-zero
    // integer prefix splits the input into separate tokens, even before . or e.
    for (prefix, accepted) in [
        ("0", true),
        ("00", true),
        ("01", true),
        ("07", true),
        ("077", true),
        ("1", true),
        ("8", true),
        ("9", true),
        ("89", true),
        ("08", false),
        ("09", false),
        ("078", false),
        ("008", false),
        ("019", false),
    ] {
        for suffix in [".5", ".85", "e1", "E+2", "e-2"] {
            for sign in ["", "-"] {
                push(
                    format!("float-{sign}{prefix}{suffix}"),
                    format!("const x :Float64 = {sign}{prefix}{suffix};"),
                    accepted,
                );
            }
        }
    }
    cases
}
