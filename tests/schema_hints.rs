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
fn every_three_node_schema_graph_has_the_correct_capability_hint() {
    // Independent transitive-closure oracle, rather than a second work-list
    // implementation. Enumerate the same 512 graphs × 8 capability placements
    // previously exported by TLC, with no JVM or frozen expected-output file.
    for graph in 0..512u16 {
        let mut reachable = [[false; 3]; 3];
        for (i, row) in reachable.iter_mut().enumerate() {
            for (j, entry) in row.iter_mut().enumerate() {
                *entry = i == j || graph & (1 << (3 * i + j)) != 0;
            }
        }
        for via in 0..3 {
            for from in 0..3 {
                for to in 0..3 {
                    reachable[from][to] |= reachable[from][via] && reachable[via][to];
                }
            }
        }
        for seeds in 0..8u8 {
            let may_contain = (0..3).any(|i| reachable[0][i] && seeds & (1 << i) != 0);
            check_graph(graph, seeds, !may_contain);
        }
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
