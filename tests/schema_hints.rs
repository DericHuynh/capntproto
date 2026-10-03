use capnp::{
    message::Builder,
    schema_capnp::{code_generator_request, node},
};
fn check_graph(graph: u16, seeds: u8, hint: bool) {
    let mut message = Builder::new_default();
    let mut nodes = message
        .init_root::<code_generator_request::Builder>()
        .init_nodes(3);
    for i in 0..3u32 {
        let mut n = nodes.reborrow().get(i);
        n.set_id(i as u64 + 1);
        n.set_display_name(format!("Node{i}"));
        let has_cap = seeds & (1 << i) != 0;
        let edges: Vec<_> = (0..3u32)
            .filter(|j| graph & (1 << (3 * i + j)) != 0)
            .collect();
        let mut fields = n
            .init_struct()
            .init_fields(edges.len() as u32 + u32::from(has_cap));
        for (field, j) in edges.iter().enumerate() {
            let mut slot = fields.reborrow().get(field as u32).init_slot();
            slot.reborrow()
                .init_type()
                .init_struct()
                .set_type_id(*j as u64 + 1);
        }
        if has_cap {
            fields
                .get(edges.len() as u32)
                .init_slot()
                .init_type()
                .init_interface()
                .set_type_id(99);
        }
    }
    let bytes = capnp::serialize::write_message_to_words(&message);
    let reader =
        capnp::serialize::read_message(std::io::Cursor::new(bytes), Default::default()).unwrap();
    let context = capnpc::codegen::GeneratorContext::new(&reader).unwrap();
    assert!(matches!(
        context.node_map[&1].which().unwrap(),
        node::Struct(_)
    ));
    assert_eq!(
        !capnpc::codegen::schema_may_contain_capabilities(&context, 1).unwrap(),
        hint,
        "graph={graph},seeds={seeds}"
    );
}
#[test]
fn replay_tlc_schema_hint_cases() {
    let path = capntproto_test_support::verification::input("CAPNTPROTO_SCHEMA_HINT_CASES")
        .expect("prepare verified trace corpus");
    let cases: Vec<serde_json::Value> =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    for c in cases {
        check_graph(
            c["graph"].as_u64().unwrap() as u16,
            c["seeds"].as_u64().unwrap() as u8,
            c["hint"].as_bool().unwrap(),
        );
    }
}
#[test]
fn recursive_schemas_terminate_without_hiding_capabilities() {
    check_graph(1, 0, true); // Self recursion without capabilities.
    check_graph(1, 1, false);
    check_graph(2 | 8, 0, true); // Mutual recursion.
    check_graph(2 | 8, 2, false);
    check_graph(2 | 32 | 256, 4, false); // Capability beyond a cycle.
}
