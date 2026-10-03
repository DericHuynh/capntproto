use crate::layout;
use crate::resolve::{AppliedAnnotation, Compilation};
use crate::source::Graph;
use crate::syntax::{AnnotationTarget as Target, FieldKind, NodeKind};
use crate::Diagnostic;
use capnp::schema_capnp::{code_generator_request, ElementSize};

pub(crate) fn compile(
    graph: &Graph,
    compiled: &Compilation,
) -> Result<capnp::message::Builder<capnp::message::HeapAllocator>, Diagnostic> {
    let layouts = layout::compile(graph, compiled)?;
    let mut encoder = crate::wire::Encoder::new(graph, compiled, &layouts);
    let nodes = &graph.nodes;
    let mut message = capnp::message::Builder::new_default();
    let mut request = message.init_root::<code_generator_request::Builder<'_>>();
    let mut files = request
        .reborrow()
        .init_requested_files(graph.requested.len() as u32);
    for (position, &index) in graph.requested.iter().enumerate() {
        let file = &graph.files[index];
        let mut out = files.reborrow().get(position as u32);
        out.set_id(nodes[file.root].id);
        out.set_filename(file.filename.as_str());
        // Value-only imports are inlined, so C++ omits them from this table.
        let mut ordered: Vec<_> = file.imports.iter().filter(|i| i.metadata).collect();
        let mut imports = out.reborrow().init_imports(ordered.len() as u32);
        // C++ writes the import table in lexical path order.
        ordered.sort_by(|a, b| a.path.cmp(&b.path));
        for (i, import) in ordered.iter().enumerate() {
            let mut out = imports.reborrow().get(i as u32);
            out.set_id(nodes[graph.files[import.target].root].id);
            out.set_name(import.path.as_str());
        }
        let references = &compiled.identifiers[index];
        let mut identifiers = out
            .init_file_source_info()
            .init_identifiers(references.len() as u32);
        for (i, &(start, end, id)) in references.iter().enumerate() {
            let mut identifier = identifiers.reborrow().get(i as u32);
            identifier.set_start_byte(start);
            identifier.set_end_byte(end);
            identifier.set_type_id(id);
        }
    }
    crate::source_info::emit(graph, compiled, request.reborrow())?;
    let mut output = request.init_nodes(compiled.selected.len() as u32);
    let mut position = 0;
    let mut display_names: Vec<String> = Vec::with_capacity(nodes.len());
    let mut display_bytes = 0;
    for (index, node) in nodes.iter().enumerate() {
        let source = graph.source(index);
        let display_name = match node.parent {
            None => node.name.clone(),
            Some(parent) => {
                let prefix = &display_names[parent];
                format!(
                    "{prefix}{}{name}",
                    if nodes[parent].parent.is_none() {
                        ':'
                    } else {
                        '.'
                    },
                    name = node.name
                )
            }
        };
        // A long enclosing name is repeated in every child's display name.
        // Bound this expansion separately from source and node counts.
        display_bytes += display_name.len();
        if display_bytes > 8 * 1024 * 1024 {
            return Err(source.error(node.span, "expanded display-name limit exceeded (8 MiB)"));
        }
        display_names.push(display_name.clone());
        if !compiled.selected.contains(&index) {
            continue;
        }
        let mut builder = output.reborrow().get(position);
        position += 1;
        builder.set_id(node.id);
        builder.set_start_byte(node.info.span.start as u32);
        builder.set_end_byte(node.info.span.end as u32);
        builder.set_is_generic(!crate::brand::Brand::lexical(nodes, index).0.is_empty());
        if !node.parameters.is_empty() {
            let mut parameters = builder
                .reborrow()
                .init_parameters(node.parameters.len() as u32);
            for (i, name) in node.parameters.iter().enumerate() {
                parameters.reborrow().get(i as u32).set_name(name.as_str());
            }
        }
        if let Some(parent) = node.parent.filter(|_| node.method.is_none()) {
            builder.set_scope_id(nodes[parent].id);
        }
        // The reference compiler uses the final dot, then a later colon, even
        // for file nodes (so "test.capnp" has prefix length 5).
        let mut prefix = display_name.rfind('.').map_or(0, |p| p + 1);
        if let Some(colon) = display_name.rfind(':') {
            if colon > prefix {
                prefix = colon + 1;
            }
        }
        builder.set_display_name_prefix_length(prefix as u32);
        builder.set_display_name(display_name.as_str());
        if !compiled.annotations[index].is_empty() {
            annotations(
                &source,
                &compiled.annotations[index],
                builder
                    .reborrow()
                    .init_annotations(compiled.annotations[index].len() as u32),
                &mut encoder,
                nodes,
            )?;
        }
        // Auxiliary group and method nodes have a null nestedNodes pointer in C++.
        if !node.is_group && node.method.is_none() {
            let mut nested = builder
                .reborrow()
                .init_nested_nodes(node.children.len() as u32);
            for (position, &child) in node.children.iter().enumerate() {
                let mut nested = nested.reborrow().get(position as u32);
                nested.set_name(nodes[child].name.as_str());
                nested.set_id(nodes[child].id);
            }
        }
        match &node.kind {
            NodeKind::File => builder.set_file(()),
            NodeKind::Const { .. } => {
                let value = compiled.constants[index].as_ref().unwrap();
                let mut output = builder.init_const();
                value.ty.emit(output.reborrow().init_type(), nodes);
                encoder
                    .emit(value, output.init_value())
                    .map_err(|e| source.error(node.span, e.to_string()))?;
            }
            NodeKind::Annotation { targets, .. } => {
                let mut output = builder.init_annotation();
                compiled.annotation_types[index]
                    .as_ref()
                    .unwrap()
                    .emit(output.reborrow().init_type(), nodes);
                output.set_targets_file(targets & Target::File.mask() != 0);
                output.set_targets_const(targets & Target::Const.mask() != 0);
                output.set_targets_enum(targets & Target::Enum.mask() != 0);
                output.set_targets_enumerant(targets & Target::Enumerant.mask() != 0);
                output.set_targets_struct(targets & Target::Struct.mask() != 0);
                output.set_targets_field(targets & Target::Field.mask() != 0);
                output.set_targets_union(targets & Target::Union.mask() != 0);
                output.set_targets_group(targets & Target::Group.mask() != 0);
                output.set_targets_interface(targets & Target::Interface.mask() != 0);
                output.set_targets_method(targets & Target::Method.mask() != 0);
                output.set_targets_param(targets & Target::Param.mask() != 0);
                output.set_targets_annotation(targets & Target::Annotation.mask() != 0);
            }
            NodeKind::Interface { methods, .. } => {
                let interface = compiled.interfaces[index].as_ref().unwrap();
                let mut output = builder.init_interface();
                let mut superclasses = output
                    .reborrow()
                    .init_superclasses(interface.superclasses.len() as u32);
                for (i, base) in interface.superclasses.iter().enumerate() {
                    let mut out = superclasses.reborrow().get(i as u32);
                    out.set_id(nodes[base.index].id);
                    if !base.brand.0.is_empty() {
                        base.brand.emit(out.init_brand(), nodes);
                    }
                }
                let mut output = output.init_methods(methods.len() as u32);
                for (i, method) in methods.iter().enumerate() {
                    let mut out = output.reborrow().get(i as u32);
                    out.set_name(method.name.as_str());
                    out.set_code_order(method.code_order);
                    let (params, results) = &interface.methods[i];
                    out.set_param_struct_type(nodes[params.index].id);
                    out.set_result_struct_type(nodes[results.index].id);
                    if !params.brand.0.is_empty() {
                        params.brand.emit(out.reborrow().init_param_brand(), nodes);
                    }
                    if !results.brand.0.is_empty() {
                        results
                            .brand
                            .emit(out.reborrow().init_result_brand(), nodes);
                    }
                    let mut implicit = out
                        .reborrow()
                        .init_implicit_parameters(method.parameters.len() as u32);
                    for (i, name) in method.parameters.iter().enumerate() {
                        implicit.reborrow().get(i as u32).set_name(name.as_str());
                    }
                    let values = &compiled.member_annotations[index][i];
                    if !values.is_empty() {
                        annotations(
                            &source,
                            values,
                            out.init_annotations(values.len() as u32),
                            &mut encoder,
                            nodes,
                        )?;
                    }
                }
            }
            NodeKind::Enum(enumerants) => {
                let mut output = builder.init_enum().init_enumerants(enumerants.len() as u32);
                for (i, enumerant) in enumerants.iter().enumerate() {
                    let mut output = output.reborrow().get(i as u32);
                    output.set_name(enumerant.name.as_str());
                    output.set_code_order(enumerant.code_order);
                    let values = &compiled.member_annotations[index][i];
                    if !values.is_empty() {
                        annotations(
                            &source,
                            values,
                            output.init_annotations(values.len() as u32),
                            &mut encoder,
                            nodes,
                        )?;
                    }
                }
            }
            NodeKind::Struct(fields) => {
                let layout = &layouts[index];
                let mut output = builder.init_struct();
                output.set_is_group(node.is_group);
                output.set_preferred_list_encoding(ElementSize::InlineComposite);
                output.set_discriminant_offset(layout.discriminant);
                output.set_discriminant_count(fields.iter().filter(|f| f.in_union).count() as u16);
                if !fields.is_empty() {
                    let mut out_fields = output.reborrow().init_fields(fields.len() as u32);
                    let mut discriminant = 0;
                    for (i, field) in fields.iter().enumerate() {
                        let mut out = out_fields.reborrow().get(i as u32);
                        out.set_name(field.name.as_str());
                        out.set_code_order(field.code_order);
                        let values = &compiled.member_annotations[index][i];
                        if !values.is_empty() {
                            annotations(
                                &source,
                                values,
                                out.reborrow().init_annotations(values.len() as u32),
                                &mut encoder,
                                nodes,
                            )?;
                        }
                        if field.in_union {
                            out.set_discriminant_value(discriminant);
                            discriminant += 1;
                        }
                        if let Some(ordinal) = field.ordinal {
                            out.reborrow().init_ordinal().set_explicit(ordinal);
                        }
                        match &field.kind {
                            FieldKind::Group(group) => {
                                out.init_group().set_type_id(nodes[*group].id);
                            }
                            FieldKind::Slot { default, .. } => {
                                let ty = compiled.fields[index].as_ref().unwrap()[i]
                                    .as_ref()
                                    .unwrap();
                                let mut slot = out.init_slot();
                                slot.set_offset(layout.offsets[i]);
                                slot.set_had_explicit_default(default.is_some());
                                ty.ty.emit(slot.reborrow().init_type(), nodes);
                                encoder
                                    .emit(ty, slot.init_default_value())
                                    .map_err(|e| source.error(field.span, e.to_string()))?;
                            }
                        }
                    }
                }
                output.set_data_word_count(layout.words);
                output.set_pointer_count(layout.pointers);
            }
        }
    }
    // capnpVersion describes the C++ executable; do not impersonate its version.
    Ok(message)
}

fn annotations(
    source: &crate::Source<'_>,
    values: &[AppliedAnnotation],
    mut output: capnp::struct_list::Builder<'_, capnp::schema_capnp::annotation::Owned>,
    encoder: &mut crate::wire::Encoder<'_>,
    nodes: &[crate::syntax::Node],
) -> Result<(), Diagnostic> {
    for (i, annotation) in values.iter().enumerate() {
        let mut out = output.reborrow().get(i as u32);
        out.set_id(nodes[annotation.declaration].id);
        annotation.brand.emit(out.reborrow().init_brand(), nodes);
        encoder
            .emit(&annotation.value, out.init_value())
            .map_err(|e| source.error(annotation.span, e.to_string()))?;
    }
    Ok(())
}
