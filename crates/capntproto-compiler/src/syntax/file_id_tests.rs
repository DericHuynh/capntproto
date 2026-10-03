use super::*;

#[test]
fn explicit_ids_required_ids_and_parse_errors_do_not_consume_entropy() {
    for text in [
        "@0xaaaaaaaaaaaaaaaa; struct S {}",
        "struct S {} @0xaaaaaaaaaaaaaaaa;",
    ] {
        for optional in [false, true] {
            let source = Source {
                filename: "explicit.capnp",
                text,
            };
            let parsed =
                parse_with_file_id(&source, optional, || panic!("unneeded entropy")).unwrap();
            assert_eq!(parsed.nodes[0].id, 0xaaaaaaaaaaaaaaaa);
        }
    }
    let source = Source {
        filename: "strict.capnp",
        text: "struct S {}",
    };
    assert!(
        parse_with_file_id(&source, false, || panic!("unneeded entropy"))
            .err()
            .unwrap()
            .message
            .contains("missing file ID")
    );
    for text in [
        "struct S {",
        "@1;",
        "@0xaaaaaaaaaaaaaaaa; @0xbbbbbbbbbbbbbbbb;",
    ] {
        let source = Source {
            filename: "invalid.capnp",
            text,
        };
        assert!(parse_with_file_id(&source, true, || panic!("unneeded entropy")).is_err());
    }
}

#[test]
fn entropy_failure_is_a_source_diagnostic_without_a_fallback_id() {
    let source = Source {
        filename: "config.capnp",
        text: "struct Config {}",
    };
    let error = parse_with_file_id(&source, true, || Err("entropy unavailable".into()))
        .err()
        .unwrap();
    assert_eq!(error.filename, "config.capnp");
    assert_eq!(
        error.message,
        "cannot generate file ID: entropy unavailable"
    );
}

#[test]
fn generated_roots_set_the_high_bit_and_use_existing_descendant_id_rules() {
    let text = "struct S { g :group { value @0 :UInt32; } struct Child {} } interface Api { call @0 (arg :S) -> (result :S); } struct Fixed @0xbbbbbbbbbbbbbbbb { struct Child {} }";
    for seed in [0, 17, u64::MAX] {
        let source = Source {
            filename: "config.capnp",
            text,
        };
        let parsed = parse_with_file_id(&source, true, || Ok(seed)).unwrap();
        let root = seed | (1 << 63);
        assert_eq!(parsed.nodes[0].id, root);
        let explicit_text = format!("{text} @0x{root:016x};");
        let explicit_source = Source {
            filename: "config.capnp",
            text: &explicit_text,
        };
        let explicit =
            parse_with_file_id(&explicit_source, false, || panic!("unneeded entropy")).unwrap();
        assert_eq!(
            parsed.nodes.iter().map(|n| n.id).collect::<Vec<_>>(),
            explicit.nodes.iter().map(|n| n.id).collect::<Vec<_>>()
        );
    }
}

#[test]
fn generated_ids_do_not_bypass_collision_checks() {
    let source = Source {
        filename: "collision.capnp",
        text: "struct S @0x8000000000000000 {}",
    };
    let error = parse_with_file_id(&source, true, || Ok(0)).err().unwrap();
    assert!(error
        .message
        .contains("duplicate schema ID 0x8000000000000000"));
}
