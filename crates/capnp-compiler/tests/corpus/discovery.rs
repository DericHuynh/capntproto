// Shared by portable tests and the pinned C++ oracle. Both spellings of leaf
// resolve to one source identity, making semantic discovery order observable.
pub const LEAF_ID: u64 = 0xdddddddddddddddd;
pub const SOURCES: &[(&str, &str)] = &[
    ("src/leaf.capnp", "@0xdddddddddddddddd; struct Leaf {} annotation note(*) :Void; const empty :Leaf = (); const number :UInt32 = 7;"),
    ("src/a.capnp", "@0xbbbbbbbbbbbbbbbb; struct Box(T) { value @0 :T; next @1 :import \"leaf.capnp\".Leaf; } struct A { value @0 :import \"leaf.capnp\".Leaf; } interface I { f @0 (value :import \"leaf.capnp\".Leaf); }"),
    ("src/b.capnp", "@0xcccccccccccccccc; struct B { value @0 :import \"/leaf.capnp\".Leaf; } interface I { f @0 (value :import \"/leaf.capnp\".Leaf); }"),
    ("src/extra.capnp", "@0xeeeeeeeeeeeeeeee; struct Extra { value @0 :import \"leaf.capnp\".Leaf; }"),
];

pub struct Case {
    pub name: &'static str,
    pub body: &'static str,
    pub leaf_name: &'static str,
    pub extra_requested: bool,
}

impl Case {
    pub fn source(&self) -> String {
        format!("@0xaaaaaaaaaaaaaaaa;\n{}\n", self.body)
    }

    pub fn requested(&self) -> Vec<&'static str> {
        let mut files = vec!["src/main.capnp"];
        if self.extra_requested {
            files.push("src/extra.capnp");
        }
        files
    }
}

pub fn cases() -> Vec<Case> {
    [
        ("used-before-unused-alias", "using Early = import \"/leaf.capnp\"; struct Root { value @0 :import \"a.capnp\".A; }", "src/leaf.capnp", false),
        ("children-source-order", "struct Z { value @0 :import \"a.capnp\".A; } struct Box(T) { value @0 :T; next @1 :import \"leaf.capnp\".Leaf; } struct A { value @0 :import \"b.capnp\".B; }", "src/leaf.capnp", false),
        ("aliases-name-order", "using Z = import \"leaf.capnp\"; using A = import \"/leaf.capnp\";", "leaf.capnp", true),
        ("nested-alias-after-child", "struct Root { using Early = import \"/leaf.capnp\"; struct Child { value @0 :import \"a.capnp\".A; } }", "src/leaf.capnp", false),
        ("fields-ordinal-order", "struct Root { later @1 :import \"b.capnp\".B; earlier @0 :import \"a.capnp\".A; }", "src/leaf.capnp", false),
        ("type-before-field-annotation", "struct Root { value @0 :import \"/leaf.capnp\".Leaf $import \"leaf.capnp\".note; }", "leaf.capnp", false),
        ("all-types-before-field-annotations", "struct Root { first @0 :Void $import \"leaf.capnp\".note; second @1 :import \"/leaf.capnp\".Leaf; }", "leaf.capnp", false),
        ("group-slots-ordinal-order", "struct Root { later @1 :import \"/leaf.capnp\".Leaf; nested :group { earlier @0 :import \"leaf.capnp\".Leaf; } }", "src/leaf.capnp", false),
        ("nested-group-slots-ordinal-order", "struct Root { later @2 :import \"/leaf.capnp\".Leaf; nested :group { middle @1 :Void; deeper :group { earlier @0 :import \"leaf.capnp\".Leaf; } } }", "src/leaf.capnp", false),
        ("inline-params-before-results", "interface Root { f @0 (value :import \"leaf.capnp\".Leaf) -> import \"/leaf.capnp\".Leaf; }", "src/leaf.capnp", false),
        ("methods-ordinal-order", "interface Root { later @1 import \"/leaf.capnp\".Leaf; earlier @0 (value :import \"leaf.capnp\".Leaf); }", "src/leaf.capnp", false),
        ("interface-bases-before-methods", "interface Root extends (import \"a.capnp\".I) { f @0 import \"b.capnp\".B; }", "src/leaf.capnp", false),
        ("pointer-defaults-after-types", "struct Root { first @0 :AnyPointer = import \"leaf.capnp\".empty; second @1 :import \"/leaf.capnp\".Leaf; }", "leaf.capnp", false),
        ("group-dependencies-source-order", "struct Root { first :group { later @1 :import \"b.capnp\".B; } second :group { earlier @0 :import \"a.capnp\".A; } }", "leaf.capnp", false),
        ("nested-group-dependencies-source-order", "struct Root { first :group { deep :group { later @1 :import \"b.capnp\".B; } } second :group { earlier @0 :import \"a.capnp\".A; } }", "leaf.capnp", false),
        ("explicit-signature-dependencies-before-auxiliary", "interface Root { f @0 (value :import \"a.capnp\".A) -> import \"b.capnp\".B; }", "leaf.capnp", false),
        ("field-annotations-source-order", "struct Root { later @1 :Void $import \"leaf.capnp\".note; earlier @0 :Void $import \"/leaf.capnp\".note; }", "src/leaf.capnp", false),
        ("node-annotations-before-pointer-defaults", "struct Root $import \"/leaf.capnp\".note { first @0 :AnyPointer = import \"leaf.capnp\".empty; }", "leaf.capnp", false),
        ("inline-defaults-after-all-method-types", "interface Root { f @0 (value :AnyPointer = import \"leaf.capnp\".empty); g @1 import \"/leaf.capnp\".Leaf; }", "leaf.capnp", false),
        ("scalar-default-before-next-type", "struct Root { first @0 :UInt32 = import \"leaf.capnp\".number; second @1 :import \"/leaf.capnp\".Leaf; }", "src/leaf.capnp", false),
        ("generic-declaration-before-brand-dependencies", "struct Root { first @0 :import \"a.capnp\".Box(import \"b.capnp\".B); }", "src/leaf.capnp", false),
        ("direct-types-before-dependencies", "struct Root { first @0 :import \"a.capnp\".A; second @1 :import \"/leaf.capnp\".Leaf; }", "leaf.capnp", false),
    ]
    .into_iter()
    .map(|(name, body, leaf_name, extra_requested)| Case { name, body, leaf_name, extra_requested })
    .collect()
}
