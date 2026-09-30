//! Bounded schema loading, rejection atomicity and owned-definition regressions.
use crate::MAX_INPUT;
use capnp::{
    any_pointer, message,
    schema_capnp::{code_generator_request, field, node},
    schema_loader::{dynamic, Kind, Limits, SchemaLoader},
};
use std::{collections::BTreeMap, sync::OnceLock};

type Builder = message::Builder<message::HeapAllocator>;

fn options() -> message::ReaderOptions {
    message::ReaderOptions {
        traversal_limit_in_words: Some(32 * 1024),
        nesting_limit: 128,
    }
}

fn limits(flags: u8) -> Limits {
    Limits {
        nodes: if flags & 1 == 0 { 16 } else { 2 },
        words: if flags & 2 == 0 { 4096 } else { 128 },
        node_words: if flags & 4 == 0 { 1024 } else { 64 },
    }
}

// A real definition and an unresolved typed dependency survive every failed
// transaction. Successful prefixes upgrade this definition before the bad node.
fn baseline(mut n: node::Builder<'_>, upgrade: bool) {
    n.set_id(1);
    n.set_display_name("Parent");
    let mut s = n.init_struct();
    s.set_pointer_count(1);
    s.set_data_word_count(u16::from(upgrade));
    let mut fields = s.init_fields(if upgrade { 2 } else { 1 });
    let mut f = fields.reborrow().get(0);
    f.set_name("child");
    let mut slot = f.init_slot();
    slot.reborrow().init_type().init_struct().set_type_id(2);
    slot.init_default_value().init_struct();
    if upgrade {
        let mut f = fields.get(1);
        f.set_name("value");
        f.set_code_order(1);
        f.reborrow().init_ordinal().set_explicit(1);
        let mut slot = f.init_slot();
        slot.reborrow().init_type().set_uint64(());
        slot.init_default_value().set_uint64(7);
    }
}

fn initial(flags: u8) -> SchemaLoader {
    let mut message = Builder::new_default();
    baseline(message.init_root(), false);
    let mut loader = SchemaLoader::new(limits(flags));
    loader.load(message.get_root_as_reader().unwrap()).unwrap();
    assert!(loader.get(2).unwrap().is_stub());
    loader
}

fn structure(n: node::Builder<'_>) {
    let mut s = n.init_struct();
    s.set_data_word_count(1);
    s.set_pointer_count(1);
    let mut fields = s.init_fields(2);
    for i in 0..2 {
        let mut f = fields.reborrow().get(i);
        f.set_name(if i == 0 { "number" } else { "text" });
        f.set_code_order(i as u16);
        f.reborrow().init_ordinal().set_explicit(i as u16);
        let mut slot = f.init_slot();
        if i == 0 {
            slot.reborrow().init_type().set_uint32(());
            slot.init_default_value().set_uint32(42);
        } else {
            slot.reborrow().init_type().set_text(());
            slot.init_default_value().set_text("default");
        }
    }
}

fn fields(n: node::Builder<'_>) -> capnp::struct_list::Builder<'_, field::Owned> {
    let node::Struct(s) = n.which().unwrap() else {
        unreachable!()
    };
    s.get_fields().unwrap()
}

fn slot(n: node::Builder<'_>, index: u32) -> field::slot::Builder<'_> {
    let field::Slot(slot) = fields(n).get(index).which().unwrap() else {
        unreachable!()
    };
    slot
}

struct Fixture {
    name: &'static str,
    bytes: Vec<u8>,
}

// Expectations describe deliberate semantic faults, independently of the
// loader's result. Unknown field types remain forward-compatible.
const CASES: &[(&str, Option<&str>)] = &[
    ("complete typed stub", None),
    ("zero node ID", Some("zero schema ID")),
    ("scalar offset overflow", Some("field offset out of bounds")),
    (
        "pointer offset overflow",
        Some("field offset out of bounds"),
    ),
    ("missing data section", Some("field offset out of bounds")),
    (
        "missing pointer section",
        Some("field offset out of bounds"),
    ),
    ("duplicate field name", Some("duplicate name")),
    ("duplicate code order", Some("invalid code order")),
    ("out of range code order", Some("invalid code order")),
    ("repeated ordinal", Some("fields not ordered by ordinal")),
    (
        "mismatched default",
        Some("field type and default mismatch"),
    ),
    ("single union arm", Some("invalid union member count")),
    (
        "union offset overflow",
        Some("union discriminant out of bounds"),
    ),
    ("duplicate union tag", Some("invalid union discriminant")),
    (
        "missing union arm",
        Some("union discriminant count mismatch"),
    ),
    ("zero dependency", Some("zero schema dependency ID")),
    (
        "conflicting dependency kinds",
        Some("conflicting dependency kinds"),
    ),
    ("recursive pointer schema", None),
    ("interface method with typed stubs", None),
    (
        "zero method parameter ID",
        Some("zero schema dependency ID"),
    ),
    ("self inheritance", Some("cyclic or overdeep")),
    ("mutual inheritance", Some("cyclic or overdeep")),
    ("enum", None),
    ("duplicate enumerant", Some("duplicate name")),
    ("constant", None),
    (
        "constant default mismatch",
        Some("field type and default mismatch"),
    ),
    ("annotation", None),
    ("file", None),
    ("pointer brand argument", None),
    (
        "duplicate brand scope",
        Some("duplicate or zero brand scope"),
    ),
    ("zero brand scope", Some("duplicate or zero brand scope")),
    (
        "scalar brand argument",
        Some("generic argument must be a pointer"),
    ),
    ("self group", Some("cyclic or overdeep")),
    ("group parent mismatch", Some("group has wrong parent")),
    ("unknown node kind", Some("was not present in the schema")),
    ("unknown field type", None),
    ("list type at depth limit", None),
    (
        "list type past depth limit",
        Some("schema type depth limit"),
    ),
];

fn fixture(index: usize) -> Fixture {
    let mut message = Builder::new_default();
    let mut nodes = message
        .init_root::<code_generator_request::Builder>()
        .init_nodes(if index == 21 { 3 } else { 2 });
    baseline(nodes.reborrow().get(0), true);
    let mut n = nodes.reborrow().get(1);
    n.set_id(4);
    n.set_display_name("Candidate");
    structure(n.reborrow());
    match index {
        0 => n.set_id(2),
        1 => n.set_id(0),
        2 | 3 => slot(n, u32::from(index == 3)).set_offset(u32::MAX),
        4 | 5 | 11..=14 => {
            let node::Struct(mut s) = n.which().unwrap() else {
                unreachable!()
            };
            match index {
                4 => s.set_data_word_count(0),
                5 => s.set_pointer_count(0),
                11 => s.set_discriminant_count(1),
                _ => {
                    s.set_discriminant_count(2);
                    s.set_discriminant_offset(if index == 12 { u32::MAX } else { 2 });
                    let mut fields = s.get_fields().unwrap();
                    fields.reborrow().get(0).set_discriminant_value(0);
                    if index != 14 {
                        fields.get(1).set_discriminant_value(u16::from(index != 13));
                    }
                }
            }
        }
        6 => fields(n).get(1).set_name("number"),
        7 | 8 => fields(n)
            .get(1)
            .set_code_order(if index == 7 { 0 } else { 2 }),
        9 => fields(n).get(1).init_ordinal().set_explicit(0),
        10 => slot(n, 0).init_default_value().set_text("wrong"),
        15 | 17 => {
            let mut slot = slot(n, 1);
            slot.reborrow()
                .init_type()
                .init_struct()
                .set_type_id(if index == 15 { 0 } else { 4 });
            slot.init_default_value().init_struct();
        }
        16 => {
            let mut slot = slot(n, 0);
            slot.reborrow().init_type().init_enum().set_type_id(2);
            slot.init_default_value().set_enum(0);
        }
        18 | 19 => {
            let mut method = n.init_interface().init_methods(1).get(0);
            method.set_name("call");
            method.set_param_struct_type(if index == 19 { 0 } else { 2 });
            method.set_result_struct_type(5);
        }
        20 | 21 => {
            n.init_interface()
                .init_superclasses(1)
                .get(0)
                .set_id(if index == 20 { 4 } else { 5 });
            if index == 21 {
                let mut other = nodes.get(2);
                other.set_id(5);
                other.init_interface().init_superclasses(1).get(0).set_id(4);
            }
        }
        22 | 23 => {
            let mut entries = n.init_enum().init_enumerants(2);
            entries.reborrow().get(0).set_name("first");
            let mut last = entries.get(1);
            last.set_name(if index == 23 { "first" } else { "last" });
            last.set_code_order(1);
        }
        24 | 25 => {
            let mut c = n.init_const();
            c.reborrow().init_type().set_uint64(());
            if index == 24 {
                c.init_value().set_uint64(42);
            } else {
                c.init_value().set_text("wrong");
            }
        }
        26 => n.init_annotation().init_type().set_text(()),
        27 => n.set_file(()),
        28..=31 => {
            let mut slot = slot(n, 1);
            slot.reborrow().init_default_value().init_struct();
            let mut ty = slot.init_type().init_struct();
            ty.set_type_id(2);
            let mut scopes = ty.init_brand().init_scopes(if index == 29 { 2 } else { 1 });
            for i in 0..scopes.len() {
                let mut scope = scopes.reborrow().get(i);
                scope.set_scope_id(if index == 30 { 0 } else { 2 });
                let mut arg = scope.init_bind(1).get(0).init_type();
                if index == 31 {
                    arg.set_uint32(());
                } else {
                    arg.set_text(());
                }
            }
        }
        32 | 33 => {
            n.set_scope_id(4);
            let node::Struct(mut s) = n.which().unwrap() else {
                unreachable!()
            };
            s.set_is_group(index == 32);
            s.get_fields().unwrap().get(1).init_group().set_type_id(4);
        }
        34 => {
            // Node's discriminant is data bytes 12..14 in schema.capnp.
            let mut raw = capnp::any_struct::Builder::from_builder(n).unwrap();
            raw.get_data_section()[12..14].copy_from_slice(&u16::MAX.to_le_bytes());
        }
        35 => {
            let mut raw = capnp::any_struct::Builder::from_builder(slot(n, 0).init_type()).unwrap();
            raw.get_data_section()[..2].copy_from_slice(&u16::MAX.to_le_bytes());
        }
        36 | 37 => {
            let mut slot = slot(n, 1);
            slot.reborrow().init_default_value().init_list();
            let mut ty = slot.init_type();
            for _ in 0..if index == 36 { 63 } else { 64 } {
                ty = ty.init_list().init_element_type();
            }
            ty.set_text(());
        }
        _ => unreachable!(),
    }
    Fixture {
        name: CASES[index].0,
        bytes: capnp::serialize::write_message_to_words(&message),
    }
}

fn fixtures() -> &'static [Fixture] {
    static FIXTURES: OnceLock<Vec<Fixture>> = OnceLock::new();
    FIXTURES.get_or_init(|| (0..CASES.len()).map(fixture).collect())
}

#[derive(Debug, PartialEq, Eq)]
struct Snapshot {
    count: usize,
    definitions: BTreeMap<u64, (bool, Vec<u8>)>,
}

fn snapshot(loader: &SchemaLoader) -> Snapshot {
    // Include the baseline dependency even while it is a stub; get_all_loaded
    // intentionally hides stubs. Count also detects leaked candidate stubs.
    let definitions = loader
        .get_all_loaded()
        .chain(loader.try_get(2))
        .map(|s| {
            let mut message = Builder::new_default();
            message.set_root(s.get_proto()).unwrap();
            let words = message.into_reader().canonicalize().unwrap();
            (
                s.id(),
                (s.is_stub(), capnp::Word::words_to_bytes(&words).to_vec()),
            )
        })
        .collect();
    Snapshot {
        count: loader.len(),
        definitions,
    }
}

fn inspect(loader: &SchemaLoader) {
    let mut empty = Builder::new_default();
    empty.init_root::<any_pointer::Builder>();
    for schema in loader.get_all_loaded() {
        let _ = schema.identity();
        let _ = schema.short_display_name();
        let _ = schema.unqualified_name();
        let _ = schema.may_contain_capabilities();
        match schema.kind() {
            Kind::Struct => {
                let reader =
                    dynamic::Reader::new(empty.get_root_as_reader().unwrap(), schema.clone())
                        .unwrap();
                let _ = reader.which();
                for f in schema.fields().unwrap().into_iter().take(16) {
                    let _ = f.identity();
                    let _ = f.get_type();
                    let _ = reader.has(f.clone());
                    let _ = reader.get(f);
                }
            }
            Kind::Interface => {
                let _ = schema.superclasses();
                let _ = schema.find_method("call");
                for method in schema.methods().unwrap().into_iter().take(16) {
                    let _ = method.identity();
                    let _ = method.params();
                    let _ = method.results();
                }
            }
            _ => (),
        }
    }
}

fn attempt(loader: &mut SchemaLoader, bytes: &[u8]) -> capnp::Result<()> {
    let message = capnp::serialize::read_message(bytes, options())?;
    loader.load_request(message.get_root()?)
}

fn check_wire(bytes: &[u8], flags: u8) -> capnp::Result<()> {
    let mut loader = initial(flags);
    let before = snapshot(&loader);
    let result = attempt(&mut loader, bytes);
    let after = snapshot(&loader);
    if result.is_err() {
        assert_eq!(
            after, before,
            "rejected schema changed existing definitions/stubs"
        );
        // A failed transaction must not poison subsequent valid loads, even if
        // it staged a replacement before a later validation failure.
        let mut recovery = Builder::new_default();
        baseline(recovery.init_root(), true);
        loader.load(recovery.get_root_as_reader().unwrap()).unwrap();
    } else {
        assert!(loader.len() <= limits(flags).nodes);
        // The original input reader has been destroyed. Exercise the owned
        // definitions, and require replay to preserve the selected versions.
        inspect(&loader);
        attempt(&mut loader, bytes).expect("accepted request failed on replay");
        assert_eq!(
            snapshot(&loader),
            after,
            "schema replay changed selected definitions"
        );
    }
    result
}

/// Header: raw/XOR/overwrite/truncate, fixture, little-endian offset, limit
/// profile (three bits). The remainder supplies raw framed requests or mutations.
pub fn check(input: &[u8]) {
    let input = &input[..input.len().min(MAX_INPUT)];
    let mode = input.first().copied().unwrap_or(0) % 4;
    let body = input.get(5..).unwrap_or_default();
    let bytes = if mode == 0 {
        body.to_vec()
    } else {
        let index = input.get(1).copied().unwrap_or(0) as usize % fixtures().len();
        let mut bytes = fixtures()[index].bytes.clone();
        let offset = u16::from_le_bytes([
            input.get(2).copied().unwrap_or(0),
            input.get(3).copied().unwrap_or(0),
        ]) as usize;
        if mode == 3 {
            bytes.truncate(offset % (bytes.len() + 1));
        } else {
            for (index, byte) in body.iter().enumerate() {
                let position = (offset + index) % bytes.len();
                if mode == 1 {
                    bytes[position] ^= byte;
                } else {
                    bytes[position] = *byte;
                }
            }
        }
        bytes
    };
    let _ = check_wire(&bytes, input.get(4).copied().unwrap_or(0));
}

pub fn seeds() -> Vec<Vec<u8>> {
    let mut seeds = vec![vec![], vec![1], vec![1, 35, 152, 2, 0, 255]];
    for (index, fixture) in fixtures().iter().enumerate() {
        assert!(fixture.bytes.len() + 5 <= MAX_INPUT, "{}", fixture.name);
        for flags in 0..8 {
            seeds.push(vec![1, index as u8, 0, 0, flags]);
        }
        let mut raw = vec![0; 5];
        raw.extend_from_slice(&fixture.bytes);
        seeds.push(raw);
        // Word boundaries cover framing, pointer tags and schema data without
        // requiring a giant initial corpus. libFuzzer mutates every byte.
        for offset in (0..fixture.bytes.len()).step_by(8) {
            let [lo, hi] = (offset as u16).to_le_bytes();
            for mode in [1, 2, 3] {
                seeds.push(vec![mode, index as u8, lo, hi, 0, 0xff]);
            }
        }
    }
    seeds.push(vec![0xff; MAX_INPUT]);
    seeds
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_type_and_default_remain_reloadable() {
        // The word-boundary sweep found an accepted future type with an unknown
        // default tag that compatibility checking subsequently rejected.
        let mut bytes = fixtures()[35].bytes.clone();
        bytes[664] ^= 255;
        check_wire(&bytes, 0).unwrap();
        check(&[1, 35, 152, 2, 0, 255]);
    }

    #[test]
    fn semantic_rejections_and_valid_controls() {
        for (fixture, (_, error)) in fixtures().iter().zip(CASES) {
            let result = check_wire(&fixture.bytes, 0);
            match *error {
                Some(reason) => assert!(
                    result
                        .as_ref()
                        .is_err_and(|error| error.to_string().contains(reason)),
                    "{}: expected {reason}, got {result:?}",
                    fixture.name
                ),
                None => result.unwrap_or_else(|error| panic!("{}: {error}", fixture.name)),
            }
        }
    }

    #[test]
    fn limits_and_rollback_oracle_have_positive_controls() {
        let baseline = initial(0);
        let before = snapshot(&baseline);
        let mut upgraded = baseline.clone();
        attempt(&mut upgraded, &fixtures()[0].bytes).unwrap();
        assert_ne!(
            snapshot(&upgraded),
            before,
            "oracle missed replacement/stub completion"
        );
        assert_eq!(upgraded.get(1).unwrap().fields().unwrap().len(), 2);
        assert!(!upgraded.get(2).unwrap().is_stub());
        assert_eq!(upgraded.get(2).unwrap().fields().unwrap().len(), 2);
        let error = check_wire(&fixtures()[18].bytes, 1).unwrap_err();
        assert!(error.to_string().contains("schema node limit"));
        let error = check_wire(&fixtures()[36].bytes, 4).unwrap_err();
        assert!(error.to_string().contains("schema size/capability limit"));
        let error = check_wire(&fixtures()[36].bytes, 2).unwrap_err();
        assert!(error.to_string().contains("schema size/capability limit"));
    }
}
