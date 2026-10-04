// Copyright (c) 2013-2015 Sandstorm Development Group, Inc. and contributors
// Licensed under the MIT License:
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
// THE SOFTWARE.

//! Schema node emission (files, structs, interfaces, enums and annotations).
use super::*;

pub(super) fn generate_node(
    ctx: &GeneratorContext,
    node_id: u64,
    node_name: &str,
) -> ::capnp::Result<FormattedText> {
    use capnp::schema_capnp::*;

    let mut output: Vec<FormattedText> = Vec::new();
    let mut nested_output: Vec<FormattedText> = Vec::new();

    let node_reader = &ctx.node_map[&node_id];
    let node_doc = ctx.documentation.formatted(node_id, None);
    let nested_nodes = node_reader.get_nested_nodes()?;
    for nested_node in nested_nodes {
        let id = nested_node.get_id();
        nested_output.push(generate_node(ctx, id, ctx.get_last_name(id)?)?);
    }

    match node_reader.which()? {
        node::File(()) => {
            output.push(documentation::file(ctx, node_id)?);
            output.push(Branch(nested_output));
        }
        node::Struct(struct_reader) => {
            let params = node_reader.parameters_texts(ctx);
            output.push(BlankLine);
            output.push(node_doc.clone());

            let is_generic = node_reader.get_is_generic();
            if is_generic {
                output.push(Line(format!(
                    "pub mod {} {{ /* {} */",
                    node_name,
                    params.expanded_list.join(",")
                )));
            } else {
                output.push(Line(format!("pub mod {node_name} {{")));
            }
            let bracketed_params = if params.params.is_empty() {
                "".to_string()
            } else {
                format!("<{}>", params.params)
            };

            let mut preamble = Vec::new();
            let mut builder_members = Vec::new();
            let mut reader_members = Vec::new();
            if ctx.field_api {
                let api = field_api::path(ctx, node_id)?;
                let args = if params.params.is_empty() {
                    String::new()
                } else {
                    format!("<{}>", params.params)
                };
                reader_members.push(Line(fmt!(ctx,"pub fn into_api(self) -> <{api}{args} as {capnp}::field_api::Schema>::Ref<'a> {{ self.reader.into() }}")));
                builder_members.push(Line(fmt!(ctx,"pub fn into_api(self) -> <{api}{args} as {capnp}::field_api::Schema>::Mut<'a> {{ self.builder.into() }}")));
            }

            let mut union_fields = Vec::new();
            let mut which_enums = Vec::new();
            let mut pipeline_impl_interior = Vec::new();
            let mut private_mod_interior = Vec::new();
            if is_generic {
                private_mod_interior.push(generate_brand(ctx, *node_reader));
            }

            let data_size = struct_reader.get_data_word_count();
            let pointer_size = struct_reader.get_pointer_count();
            let discriminant_count = struct_reader.get_discriminant_count();
            let discriminant_offset = struct_reader.get_discriminant_offset();

            private_mod_interior.push(crate::pointer_constants::node_word_array_declaration(
                ctx,
                "ENCODED_NODE",
                *node_reader,
                crate::pointer_constants::WordArrayDeclarationOptions { pub_crate: true },
            )?);

            private_mod_interior.push(generate_get_field_types(ctx, *node_reader)?);
            private_mod_interior.push(generate_get_annotation_types(ctx, *node_reader)?);

            // `static` instead of `const` so that this has a fixed memory address
            // and we can check equality of `RawStructSchema` values by comparing pointers.
            private_mod_interior.push(Branch(vec![
                Line(fmt!(ctx, "pub(crate) static ARENA: {capnp}::private::arena::GeneratedCodeArena = {capnp}::private::arena::GeneratedCodeArena::new(&ENCODED_NODE);")),
                Line(fmt!(ctx,"pub(crate) static RAW_SCHEMA: {capnp}::introspect::RawStructSchema = {capnp}::introspect::RawStructSchema::new(")),
                indent(vec![
                    Line("&ARENA,".into()),
                    Line("NONUNION_MEMBERS,".into()),
                    Line("MEMBERS_BY_DISCRIMINANT,".into()),
                    Line("MEMBERS_BY_NAME".into()),
                ]),
                Line(");".into()),
            ]));

            private_mod_interior.push(generate_members_by_discriminant(*node_reader)?);
            private_mod_interior.push(generate_members_by_name(*node_reader)?);

            let mut has_pointer_field = false;
            let fields = struct_reader.get_fields()?;
            for (index, field) in fields.iter().enumerate() {
                let doc = ctx.documentation.formatted(node_id, Some(index));
                let name = get_field_name(field)?;
                let styled_name = camel_to_snake_case(name);

                let discriminant_value = field.get_discriminant_value();
                let is_union_field = discriminant_value != field::NO_DISCRIMINANT;

                match field.which()? {
                    field::Slot(s) => match s.get_type()?.which()? {
                        type_::Text(())
                        | type_::Data(())
                        | type_::List(_)
                        | type_::Struct(_)
                        | type_::Interface(_)
                        | type_::AnyPointer(_) => has_pointer_field = true,
                        _ => (),
                    },
                    field::Group(_) => has_pointer_field = true,
                }

                if !is_union_field {
                    pipeline_impl_interior.push(documentation::attach(
                        &doc,
                        generate_pipeline_getter(ctx, field)?,
                    ));
                    let (ty, get, default_decl) = getter_text(ctx, &field, true, true)?;
                    if let Some(default) = default_decl {
                        private_mod_interior.push(default.clone());
                    }
                    reader_members.push(Branch(vec![
                        doc.clone(),
                        line("#[inline]"),
                        Line(format!("pub fn get_{styled_name}(self) {ty} {{")),
                        indent(get),
                        line("}"),
                    ]));

                    let (ty_b, get_b, _) = getter_text(ctx, &field, false, true)?;
                    builder_members.push(Branch(vec![
                        doc.clone(),
                        line("#[inline]"),
                        Line(format!("pub fn get_{styled_name}(self) {ty_b} {{")),
                        indent(get_b),
                        line("}"),
                    ]));
                } else {
                    union_fields.push((index, field));
                }

                builder_members.push(generate_setter(
                    ctx,
                    discriminant_offset,
                    &styled_name,
                    &field,
                    &doc,
                )?);

                reader_members.push(documentation::attach(
                    &doc,
                    generate_haser(discriminant_offset, &styled_name, &field, true)?,
                ));
                builder_members.push(documentation::attach(
                    &doc,
                    generate_haser(discriminant_offset, &styled_name, &field, false)?,
                ));

                if let Ok(field::Group(group)) = field.which() {
                    let id = group.get_type_id();
                    let text = generate_node(ctx, id, ctx.get_last_name(id)?)?;
                    nested_output.push(text);
                }
            }

            if discriminant_count > 0 {
                let (which_enums1, union_getter, typedef, mut default_decls) = generate_union(
                    ctx,
                    node_id,
                    discriminant_offset,
                    &union_fields,
                    true,
                    &params,
                )?;
                which_enums.push(which_enums1);
                which_enums.push(typedef);
                reader_members.push(union_getter);

                private_mod_interior.append(&mut default_decls);

                let (_, union_getter, typedef, _) = generate_union(
                    ctx,
                    node_id,
                    discriminant_offset,
                    &union_fields,
                    false,
                    &params,
                )?;
                which_enums.push(typedef);
                builder_members.push(union_getter);

                let mut reexports = String::new();
                reexports.push_str("pub use self::Which::{");
                let mut whichs = Vec::new();
                for (_, f) in &union_fields {
                    whichs.push(capitalize_first_letter(get_field_name(*f)?));
                }
                reexports.push_str(&whichs.join(","));
                reexports.push_str("};");
                preamble.push(Line(reexports));
                preamble.push(BlankLine);
            }

            let builder_struct_size =
                Branch(vec![
                    Line(fmt!(ctx,"impl <{0}> {capnp}::traits::HasStructSize for Builder<'_,{0}> {1} {{",
                                 params.params, params.where_clause)),
                                 indent(Line(
                        fmt!(ctx,"const STRUCT_SIZE: {capnp}::private::layout::StructSize = {capnp}::private::layout::StructSize {{ data: {}, pointers: {} }};", data_size as usize, pointer_size as usize))),
                   line("}")]);

            private_mod_interior.push(Line(format!(
                "pub(crate) const TYPE_ID: u64 = {};",
                format_u64(node_id)
            )));

            let from_pointer_builder_impl =
                Branch(vec![
                    Line(fmt!(ctx,"impl <'a,{0}> {capnp}::traits::FromPointerBuilder<'a> for Builder<'a,{0}> {1} {{", params.params, params.where_clause)),
                    indent(vec![
                        Line(fmt!(ctx,"fn init_pointer(builder: {capnp}::private::layout::PointerBuilder<'a>, _size: u32) -> Self {{")),
                        indent(Line(fmt!(ctx,"builder.init_struct(<Self as {capnp}::traits::HasStructSize>::STRUCT_SIZE).into()"))),
                        line("}"),
                        Line(fmt!(ctx,"fn get_from_pointer(builder: {capnp}::private::layout::PointerBuilder<'a>, default: ::core::option::Option<&'a [{capnp}::Word]>) -> {capnp}::Result<Self> {{")),
                        indent(Line(fmt!(ctx,"::core::result::Result::Ok(builder.get_struct(<Self as {capnp}::traits::HasStructSize>::STRUCT_SIZE, default)?.into())"))),
                        line("}")
                    ]),
                    line("}"),
                    BlankLine]);

            let accessors = vec![
                Branch(preamble),
                node_doc.clone(),
                (if !is_generic {
                    Branch(vec![
                        Line("#[derive(Copy, Clone)]".into()),
                        line("pub struct Owned(());"),
                        Line(fmt!(ctx,"impl {capnp}::introspect::Introspect for Owned {{ fn introspect() -> {capnp}::introspect::Type {{ {capnp}::introspect::TypeVariant::Struct({capnp}::introspect::RawBrandedStructSchema {{ generic: &_private::RAW_SCHEMA, field_types: _private::get_field_types, annotation_types: _private::get_annotation_types }}).into() }} }}")),
                        Line(fmt!(ctx, "impl {capnp}::traits::Owned for Owned {{ type Reader<'a> = Reader<'a>; type Builder<'a> = Builder<'a>; }}")),
                        Line(fmt!(ctx,"impl {capnp}::traits::OwnedStruct for Owned {{ type Reader<'a> = Reader<'a>; type Builder<'a> = Builder<'a>; }}")),
                        Line(fmt!(ctx,"impl {capnp}::traits::Pipelined for Owned {{ type Pipeline = Pipeline; }}"))
                    ])
                } else {
                    Branch(vec![
                        Line("#[derive(Copy, Clone)]".into()),
                        Line(format!("pub struct Owned<{}> {{", params.params)),
                            indent(Line(params.phantom_data_type.clone())),
                        line("}"),
                        Line(fmt!(ctx,"impl <{0}> {capnp}::introspect::Introspect for Owned <{0}> {1} {{ fn introspect() -> {capnp}::introspect::Type {{ {capnp}::introspect::TypeVariant::Struct({capnp}::introspect::RawBrandedStructSchema {{ generic: &_private::RAW_SCHEMA, field_types: _private::get_field_types::<{0}>, annotation_types: _private::get_annotation_types::<{0}> }}).into_type_with_brand({capnp}::introspect::Brand::new({2}, _private::get_brand_parameter::<{0}>)) }} }}",
                            params.params, params.where_clause, params.expanded_list.len())),
                        Line(fmt!(ctx,"impl <{0}> {capnp}::traits::Owned for Owned <{0}> {1} {{ type Reader<'a> = Reader<'a, {0}>; type Builder<'a> = Builder<'a, {0}>; }}",
                            params.params, params.where_clause)),
                        Line(fmt!(ctx,"impl <{0}> {capnp}::traits::OwnedStruct for Owned <{0}> {1} {{ type Reader<'a> = Reader<'a, {0}>; type Builder<'a> = Builder<'a, {0}>; }}",
                            params.params, params.where_clause)),
                        Line(fmt!(ctx,"impl <{0}> {capnp}::traits::Pipelined for Owned<{0}> {1} {{ type Pipeline = Pipeline{2}; }}",
                            params.params, params.where_clause, bracketed_params)),
                    ])
                }),
                BlankLine,
                node_doc.clone(),
                (if !is_generic {
                    Line(fmt!(ctx,"pub struct Reader<'a> {{ reader: {capnp}::private::layout::StructReader<'a> }}"))
                } else {
                    Branch(vec![
                        Line(format!("pub struct Reader<'a,{}> {} {{", params.params, params.where_clause)),
                        indent(vec![
                            Line(fmt!(ctx,"reader: {capnp}::private::layout::StructReader<'a>,")),
                            Line(params.phantom_data_type.clone()),
                        ]),
                        line("}")
                    ])
                }),
                // Manually implement Copy/Clone because `derive` only kicks in if all of
                // the parameters are known to implement Copy/Clone.
                Branch(vec![
                    Line(format!("impl <{0}> ::core::marker::Copy for Reader<'_,{0}> {1} {{}}",
                                 params.params, params.where_clause)),
                    Line(format!("impl <{0}> ::core::clone::Clone for Reader<'_,{0}> {1} {{",
                                 params.params, params.where_clause)),
                    indent(Line("fn clone(&self) -> Self { *self }".into())),
                    Line("}".into())]),
                BlankLine,
                Branch(vec![
                        Line(fmt!(ctx,"impl <{0}> {capnp}::traits::HasTypeId for Reader<'_,{0}> {1} {{",
                            params.params, params.where_clause)),
                        indent(vec![line("const TYPE_ID: u64 = _private::TYPE_ID;")]),
                    line("}")]),
                Line(fmt!(ctx,"impl <'a,{0}> ::core::convert::From<{capnp}::private::layout::StructReader<'a>> for Reader<'a,{0}> {1} {{",
                            params.params, params.where_clause)),
                indent(vec![
                    Line(fmt!(ctx,"fn from(reader: {capnp}::private::layout::StructReader<'a>) -> Self {{")),
                    indent(Line(format!("Self {{ reader, {} }}", params.phantom_data_value))),
                    line("}")
                ]),
                line("}"),
                BlankLine,
                Line(fmt!(ctx,"impl <'a,{0}> ::core::convert::From<Reader<'a,{0}>> for {capnp}::dynamic_value::Reader<'a> {1} {{",
                            params.params, params.where_clause)),
                indent(vec![
                    Line(format!("fn from(reader: Reader<'a,{0}>) -> Self {{", params.params)),
                    indent(Line(fmt!(ctx,"Self::Struct({capnp}::dynamic_struct::Reader::new(reader.reader, <Owned<{0}> as {capnp}::introspect::Introspect>::introspect().as_struct_schema().unwrap()))", params.params))),
                    line("}")
                ]),
                line("}"),
                BlankLine,
                Line(format!("impl <{0}> ::core::fmt::Debug for Reader<'_,{0}> {1} {{",
                            params.params, params.where_clause)),
                indent(vec![
                    Line("fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::result::Result<(), ::core::fmt::Error> {".into()),
                    indent(Line(fmt!(ctx,"core::fmt::Debug::fmt(&::core::convert::Into::<{capnp}::dynamic_value::Reader<'_>>::into(*self), f)"))),
                    line("}")
                ]),
                line("}"),

                BlankLine,

                Line(fmt!(ctx,"impl <'a,{0}> {capnp}::traits::FromPointerReader<'a> for Reader<'a,{0}> {1} {{",
                    params.params, params.where_clause)),
                indent(vec![
                    Line(fmt!(ctx,"fn get_from_pointer(reader: &{capnp}::private::layout::PointerReader<'a>, default: ::core::option::Option<&'a [{capnp}::Word]>) -> {capnp}::Result<Self> {{")),
                    indent(line("::core::result::Result::Ok(reader.get_struct(default)?.into())")),
                    line("}")
                ]),
                line("}"),
                BlankLine,
                Line(fmt!(ctx,"impl <'a,{0}> {capnp}::traits::IntoInternalStructReader<'a> for Reader<'a,{0}> {1} {{",
                            params.params, params.where_clause)),
                indent(vec![
                    Line(fmt!(ctx,"fn into_internal_struct_reader(self) -> {capnp}::private::layout::StructReader<'a> {{")),
                    indent(line("self.reader")),
                    line("}")
                ]),
                line("}"),
                BlankLine,
                Line(fmt!(ctx,"impl <'a,{0}> {capnp}::traits::Imbue<'a> for Reader<'a,{0}> {1} {{",
                    params.params, params.where_clause)),
                indent(vec![
                    Line(fmt!(ctx,"fn imbue(&mut self, cap_table: &'a {capnp}::private::layout::CapTable) {{")),
                    indent(Line(fmt!(ctx,"self.reader.imbue({capnp}::private::layout::CapTableReader::Plain(cap_table))"))),
                    line("}")
                ]),
                line("}"),
                BlankLine,
                if has_pointer_field || ctx.field_api { // the facade bridge returns a borrowed reader
                    Line(format!("impl <'a,{0}> Reader<'a,{0}> {1} {{", params.params, params.where_clause))
                } else {
                    Line(format!("impl <{0}> Reader<'_,{0}> {1} {{", params.params, params.where_clause))
                },
                indent(vec![
                        Line(format!("pub fn reborrow(&self) -> Reader<'_,{}> {{",params.params)),
                        indent(line("Self { .. *self }")),
                        line("}"),
                        BlankLine,
                        Line(fmt!(ctx,"pub fn total_size(&self) -> {capnp}::Result<{capnp}::MessageSize> {{")),
                        indent(line("self.reader.total_size()")),
                        line("}")]),
                indent(reader_members),
                line("}"),
                BlankLine,
                node_doc.clone(),
                (if !is_generic {
                    Line(fmt!(ctx,"pub struct Builder<'a> {{ builder: {capnp}::private::layout::StructBuilder<'a> }}"))
                } else {
                    Branch(vec![
                        Line(format!("pub struct Builder<'a,{}> {} {{",
                                     params.params, params.where_clause)),
                            indent(vec![
                            Line(fmt!(ctx, "builder: {capnp}::private::layout::StructBuilder<'a>,")),
                            Line(params.phantom_data_type.clone()),
                        ]),
                        line("}")
                    ])
                }),
                builder_struct_size,
                Branch(vec![
                    Line(fmt!(ctx,"impl <{0}> {capnp}::traits::HasTypeId for Builder<'_,{0}> {1} {{",
                                 params.params, params.where_clause)),
                    indent(vec![
                        line("const TYPE_ID: u64 = _private::TYPE_ID;")]),
                    line("}")
                ]),
                Line(fmt!(ctx,
                    "impl <'a,{0}> ::core::convert::From<{capnp}::private::layout::StructBuilder<'a>> for Builder<'a,{0}> {1} {{",
                    params.params, params.where_clause)),
                indent(vec![
                        Line(fmt!(ctx,"fn from(builder: {capnp}::private::layout::StructBuilder<'a>) -> Self {{")),
                        indent(Line(format!("Self {{ builder, {} }}", params.phantom_data_value))),
                        line("}")
                ]),
                line("}"),
                BlankLine,
                Line(fmt!(ctx,"impl <'a,{0}> ::core::convert::From<Builder<'a,{0}>> for {capnp}::dynamic_value::Builder<'a> {1} {{",
                            params.params, params.where_clause)),
                indent(vec![
                        Line(format!("fn from(builder: Builder<'a,{0}>) -> Self {{", params.params)),
                        indent(Line(fmt!(ctx,"Self::Struct({capnp}::dynamic_struct::Builder::new(builder.builder, <Owned<{0}> as {capnp}::introspect::Introspect>::introspect().as_struct_schema().unwrap()))", params.params))),
                        line("}")
                ]),
                line("}"),
                BlankLine,

                Line(fmt!(ctx,"impl <'a,{0}> {capnp}::traits::ImbueMut<'a> for Builder<'a,{0}> {1} {{",
                             params.params, params.where_clause)),
                indent(vec![
                        Line(fmt!(ctx,"fn imbue_mut(&mut self, cap_table: &'a mut {capnp}::private::layout::CapTable) {{")),
                        indent(Line(fmt!(ctx,"self.builder.imbue({capnp}::private::layout::CapTableBuilder::Plain(cap_table))"))),
                        line("}")]),
                line("}"),
                BlankLine,

                from_pointer_builder_impl,
                Line(fmt!(ctx,
                    "impl <{0}> {capnp}::traits::SetterInput<Owned<{0}>> for Reader<'_,{0}> {1} {{",
                    params.params, params.where_clause)),
                indent(Line(fmt!(ctx,"fn set_pointer_builder(mut pointer: {capnp}::private::layout::PointerBuilder<'_>, value: Self, canonicalize: bool) -> {capnp}::Result<()> {{ pointer.set_struct(&value.reader, canonicalize) }}"))),
                line("}"),
                BlankLine,
                Line(format!("impl <'a,{0}> Builder<'a,{0}> {1} {{", params.params, params.where_clause)),
                indent(vec![
                        Line(format!("pub fn into_reader(self) -> Reader<'a,{}> {{", params.params)),
                        indent(line("self.builder.into_reader().into()")),
                        line("}"),
                        Line(format!("pub fn reborrow(&mut self) -> Builder<'_,{}> {{", params.params)),
                        (if !is_generic {
                            indent(line("Builder { builder: self.builder.reborrow() }"))
                        } else {
                            indent(line("Builder { builder: self.builder.reborrow(), ..*self }"))
                        }),
                        line("}"),
                        Line(format!("pub fn reborrow_as_reader(&self) -> Reader<'_,{}> {{", params.params)),
                        indent(line("self.builder.as_reader().into()")),
                        line("}"),

                        BlankLine,
                        Line(fmt!(ctx,"pub fn total_size(&self) -> {capnp}::Result<{capnp}::MessageSize> {{")),
                        indent(line("self.builder.as_reader().total_size()")),
                        line("}")
                        ]),
                indent(builder_members),
                line("}"),
                BlankLine,
                (if is_generic {
                    Branch(vec![
                        Line(format!("pub struct Pipeline{bracketed_params} {{")),
                        indent(vec![
                            Line(fmt!(ctx,"_typeless: {capnp}::any_pointer::Pipeline,")),
                            Line(params.phantom_data_type),
                        ]),
                        line("}")
                    ])
                } else {
                    Line(fmt!(ctx,"pub struct Pipeline {{ _typeless: {capnp}::any_pointer::Pipeline }}"))
                }),
                Line(fmt!(ctx,"impl{bracketed_params} {capnp}::capability::FromTypelessPipeline for Pipeline{bracketed_params} {{")),
                indent(vec![
                        Line(fmt!(ctx,"fn new(typeless: {capnp}::any_pointer::Pipeline) -> Self {{")),
                        indent(Line(format!("Self {{ _typeless: typeless, {} }}", params.phantom_data_value))),
                        line("}")]),
                line("}"),
                Line(fmt!(ctx,"impl{bracketed_params} {capnp}::capability::IntoTypelessPipeline for Pipeline{bracketed_params} {{")),
                indent(vec![
                    Line(fmt!(ctx,"fn into_typeless_pipeline(self) -> {capnp}::any_pointer::Pipeline {{")),
                    indent(line("self._typeless")),
                    line("}")]),
                line("}"),
                Line(format!("impl{0} Pipeline{0} {1} {{", bracketed_params,
                             params.pipeline_where_clause)),
                indent(pipeline_impl_interior),
                line("}"),
                line("mod _private {"),
                indent(private_mod_interior),
                line("}"),
            ];

            output.push(indent(vec![
                Branch(accessors),
                Branch(which_enums),
                Branch(nested_output),
            ]));
            output.push(line("}"));
        }

        node::Enum(enum_reader) => {
            let last_name = ctx.get_last_name(node_id)?;
            let name_as_mod = module_name(last_name);
            // Native enum values erase their enclosing type parameters, as in
            // C++. Their annotation callbacks therefore use default arguments.
            let params = node_reader.parameters_texts(ctx);
            let defaults = params
                .expanded_list
                .iter()
                .map(|_| fmt!(ctx, "{capnp}::any_pointer::Owned"))
                .collect::<Vec<_>>()
                .join(",");
            let annotations = if defaults.is_empty() {
                format!("{name_as_mod}::get_annotation_types")
            } else {
                format!("{name_as_mod}::get_annotation_types::<{defaults}>")
            };
            output.push(BlankLine);

            let mut members = Vec::new();
            let mut match_branches = Vec::new();
            let enumerants = enum_reader.get_enumerants()?;
            for (ii, enumerant) in enumerants.into_iter().enumerate() {
                members.push(ctx.documentation.formatted(node_id, Some(ii)));
                let enumerant = capitalize_first_letter(get_enumerant_name(enumerant)?);
                members.push(Line(format!("{enumerant} = {ii},")));
                match_branches.push(Line(format!(
                    "{ii} => ::core::result::Result::Ok(Self::{enumerant}),"
                )));
            }
            match_branches.push(Line(fmt!(
                ctx,
                "n => ::core::result::Result::Err({capnp}::NotInSchema(n)),"
            )));

            output.push(Branch(vec![
                node_doc.clone(),
                line("#[repr(u16)]"),
                line("#[derive(Clone, Copy, Debug, PartialEq, Eq)]"),
                Line(format!("pub enum {last_name} {{")),
                indent(members),
                line("}"),
            ]));

            output.push(BlankLine);
            output.push(Branch(vec![
                Line(fmt!(ctx,
                    "impl {capnp}::introspect::Introspect for {last_name} {{"
                )),
                indent(Line(fmt!(ctx,
                    "fn introspect() -> {capnp}::introspect::Type {{ {capnp}::introspect::TypeVariant::Enum({capnp}::introspect::RawEnumSchema {{ encoded_node: &{name_as_mod}::ENCODED_NODE, annotation_types: {annotations} }}).into() }}"))),
                Line("}".into()),
            ]));

            output.push(Branch(vec![
                Line(fmt!(ctx,"impl ::core::convert::From<{last_name}> for {capnp}::dynamic_value::Reader<'_> {{")),
                indent(Line(fmt!(ctx,
                    "fn from(e: {last_name}) -> Self {{ {capnp}::dynamic_value::Enum::new(e.into(), {capnp}::introspect::RawEnumSchema {{ encoded_node: &{name_as_mod}::ENCODED_NODE, annotation_types: {annotations} }}.into()).into() }}"))),
                Line("}".into())
            ]));

            output.push(Branch(vec![
                Line(format!(
                    "impl ::core::convert::TryFrom<u16> for {last_name} {{"
                )),
                indent(Line(
                    fmt!(ctx,"type Error = {capnp}::NotInSchema;"),
                )),
                indent(vec![
                    Line(
                        format!("fn try_from(value: u16) -> ::core::result::Result<Self, <{last_name} as ::core::convert::TryFrom<u16>>::Error> {{")
                    ),
                    indent(vec![
                        line("match value {"),
                        indent(match_branches),
                        line("}"),
                    ]),
                    line("}"),
                ]),
                line("}"),
                Line(format!("impl From<{last_name}> for u16 {{")),
                indent(line("#[inline]")),
                indent(Line(format!(
                    "fn from(x: {last_name}) -> u16 {{ x as u16 }}"
                ))),
                line("}"),
            ]));

            output.push(Branch(vec![
                Line(fmt!(
                    ctx,
                    "impl {capnp}::traits::HasTypeId for {last_name} {{"
                )),
                indent(Line(format!(
                    "const TYPE_ID: u64 = {}u64;",
                    format_u64(node_id)
                ))),
                line("}"),
            ]));

            output.push(Branch(vec![
                Line(format!("mod {name_as_mod} {{")),
                Branch(vec![
                    crate::pointer_constants::node_word_array_declaration(
                        ctx,
                        "ENCODED_NODE",
                        *node_reader,
                        crate::pointer_constants::WordArrayDeclarationOptions { pub_crate: true },
                    )?,
                    generate_get_annotation_types(ctx, *node_reader)?,
                ]),
                Line("}".into()),
            ]));
        }

        node::Interface(interface) => {
            let params = node_reader.parameters_texts(ctx);
            output.push(BlankLine);

            let is_generic = node_reader.get_is_generic();

            let names = &ctx.scope_map[&node_id];
            let mut client_impl_interior = Vec::new();
            let mut server_interior = Vec::new();
            let mut mod_interior = Vec::new();
            let mut dispatch_arms = Vec::new();
            let mut method_type_arms = Vec::new();
            let mut private_mod_interior = vec![generate_brand(ctx, *node_reader)];

            let bracketed_params = if params.params.is_empty() {
                "".to_string()
            } else {
                format!("<{}>", params.params)
            };

            private_mod_interior.push(Line(format!(
                "pub(crate) const TYPE_ID: u64 = {};",
                format_u64(node_id)
            )));

            mod_interior.push(line("#![allow(unused_variables)]"));

            let methods = interface.get_methods()?;
            for (ordinal, method) in methods.into_iter().enumerate() {
                let doc = ctx.documentation.formatted(node_id, Some(ordinal));
                server_interior.push(doc.clone());
                client_impl_interior.push(doc.clone());
                // Match the pinned C++ generator: method, declaring interface,
                // or containing file may opt into post-dispatch cancellation.
                const ALLOW_CANCELLATION: u64 = 0xac7096ff8cfc9dce;
                let mut file = *node_reader;
                while file.get_scope_id() != 0 {
                    file = ctx.node_map[&file.get_scope_id()];
                }
                let allow_cancellation = method
                    .get_annotations()?
                    .iter()
                    .chain(node_reader.get_annotations()?.iter())
                    .chain(file.get_annotations()?.iter())
                    .any(|annotation| annotation.get_id() == ALLOW_CANCELLATION);

                let name = method.get_name()?.to_str()?;

                let param_id = method.get_param_struct_type();
                let param_node = &ctx.node_map[&param_id];
                let (param_scopes, params_ty_params) = if param_node.get_scope_id() == 0 {
                    let mut names = names.clone();
                    let local_name = module_name(&format!("{name}Params"));
                    nested_output.push(generate_node(ctx, param_id, &local_name)?);
                    names.push(local_name);
                    (names, params.params.clone())
                } else {
                    (
                        ctx.scope_map[&param_node.get_id()].clone(),
                        get_ty_params_of_brand(
                            ctx,
                            method.get_param_brand()?,
                            node_reader.get_id(),
                        )?,
                    )
                };
                let param_type = do_branding(
                    ctx,
                    param_id,
                    method.get_param_brand()?,
                    Leaf::Owned,
                    &param_scopes.join("::"),
                )?;

                mod_interior.push(Line(fmt!(
                    ctx,
                    "pub type {}Params<{}> = {capnp}::capability::Params<{}>;",
                    capitalize_first_letter(name),
                    params_ty_params,
                    param_type
                )));

                let result_id = method.get_result_struct_type();
                let no_promise_pipelining = !schema_may_contain_capabilities(ctx, result_id)?;
                if result_id != STREAM_RESULT_ID {
                    let (reply_type, reply_conversion) = if ctx.structured_replies {
                        ("Reply", "internal_get_typed_reply")
                    } else {
                        ("Results", "internal_get_typed_results")
                    };
                    dispatch_arms.push(
                        Line(fmt!(ctx,
                                  "{ordinal} => {capnp}::capability::DispatchCallResult::with_cancellation_policy({capnp}::capability::Promise::from_future(<_T as Server{bracketed_params}>::{}(this, {capnp}::private::capability::internal_get_typed_params(params), {capnp}::private::capability::{reply_conversion}(results))), false, {allow_cancellation}),",
                                  module_name(name))));

                    let result_node = &ctx.node_map[&result_id];
                    let (result_scopes, results_ty_params) = if result_node.get_scope_id() == 0 {
                        let mut names = names.clone();
                        let local_name = module_name(&format!("{name}Results"));
                        nested_output.push(generate_node(ctx, result_id, &local_name)?);
                        names.push(local_name);
                        (names, params.params.clone())
                    } else {
                        (
                            ctx.scope_map[&result_node.get_id()].clone(),
                            get_ty_params_of_brand(
                                ctx,
                                method.get_result_brand()?,
                                node_reader.get_id(),
                            )?,
                        )
                    };
                    let result_type = do_branding(
                        ctx,
                        result_id,
                        method.get_result_brand()?,
                        Leaf::Owned,
                        &result_scopes.join("::"),
                    )?;
                    method_type_arms.push(Line(fmt!(ctx, "{ordinal} => {capnp}::introspect::MethodTypes {{ params: <{param_type} as {capnp}::introspect::Introspect>::introspect(), results: ::core::option::Option::Some(<{result_type} as {capnp}::introspect::Introspect>::introspect()), no_promise_pipelining: {no_promise_pipelining} }},")));
                    mod_interior.push(Line(fmt!(
                        ctx,
                        "pub type {}Results<{}> = {capnp}::capability::{reply_type}<{}>;",
                        capitalize_first_letter(name),
                        results_ty_params,
                        result_type
                    )));
                    server_interior.push(
                        Line(fmt!(ctx,
                                  "fn {}(self: {capnp}::capability::Rc<Self>, _: {}Params<{}>, _: {}Results<{}>) -> impl ::core::future::Future<Output = Result<(), {capnp}::Error>> + 'static {{ ::core::future::ready(Err({capnp}::Error::unimplemented(\"method {}::Server::{} not implemented\".to_string()))) }}",
                                  module_name(name),
                                  capitalize_first_letter(name), params_ty_params,
                                  capitalize_first_letter(name), results_ty_params,
                                  node_name, module_name(name)
                        )));

                    client_impl_interior.push(Line(fmt!(
                        ctx,
                        "pub fn {}_request(&self) -> {capnp}::capability::Request<{},{}> {{",
                        camel_to_snake_case(name),
                        param_type,
                        result_type
                    )));

                    client_impl_interior.push(indent(Line(format!(
                        "self.client.new_call_with_hints(_private::TYPE_ID, {ordinal}, ::core::option::Option::None, {}::capability::CallHints {{ no_promise_pipelining: {no_promise_pipelining}, only_promise_pipeline: false }})", ctx.capnp_root
                    ))));
                    client_impl_interior.push(line("}"));
                    if ctx.field_api {
                        let p = field_api::remap(ctx, param_type.clone())?;
                        let r = field_api::remap(ctx, result_type.clone())?;
                        let method = camel_to_snake_case(name);
                        client_impl_interior.push(doc.clone());
                        client_impl_interior.push(Line(fmt!(ctx,"pub fn {method}_call(&self) -> {capnp}::field_api::Request<{p},{r}> {{ {capnp}::field_api::Request::new(self.{method}_request().hook) }}")));
                    }
                } else {
                    method_type_arms.push(Line(fmt!(ctx, "{ordinal} => {capnp}::introspect::MethodTypes {{ params: <{param_type} as {capnp}::introspect::Introspect>::introspect(), results: ::core::option::Option::None, no_promise_pipelining: true }},")));
                    // It's a streaming method.
                    dispatch_arms.push(
                        Line(fmt!(ctx,
                                  "{ordinal} => {capnp}::capability::DispatchCallResult::with_cancellation_policy({capnp}::capability::Promise::from_future(<_T as Server{bracketed_params}>::{}(this, {capnp}::private::capability::internal_get_typed_params(params))), true, {allow_cancellation}),",

                                  module_name(name))));

                    server_interior.push(
                        Line(fmt!(ctx,
                                  "fn {}(self: {capnp}::capability::Rc<Self>, _: {}Params<{}>) -> impl ::core::future::Future<Output = Result<(), {capnp}::Error>> + 'static {{ ::core::future::ready(Err({capnp}::Error::unimplemented(\"method {}::Server::{} not implemented\".to_string()))) }}",
                                  module_name(name),
                                  capitalize_first_letter(name), params_ty_params,
                                  node_name, module_name(name)
                        )));
                    client_impl_interior.push(Line(fmt!(
                        ctx,
                        "pub fn {}_request(&self) -> {capnp}::capability::StreamingRequest<{}> {{",
                        camel_to_snake_case(name),
                        param_type
                    )));
                    client_impl_interior.push(indent(Line(format!(
                        "self.client.new_streaming_call_with_hints(_private::TYPE_ID, {ordinal}, ::core::option::Option::None, {}::capability::CallHints {{ no_promise_pipelining: true, only_promise_pipeline: false }})", ctx.capnp_root
                    ))));

                    client_impl_interior.push(line("}"));
                    if ctx.field_api {
                        let p = field_api::remap(ctx, param_type.clone())?;
                        let method = camel_to_snake_case(name);
                        client_impl_interior.push(doc.clone());
                        client_impl_interior.push(Line(fmt!(ctx,"pub fn {method}_call(&self) -> {capnp}::field_api::StreamingRequest<{p}> {{ {capnp}::field_api::StreamingRequest::new(self.{method}_request().hook) }}")));
                    }
                }

                method.get_annotations()?;
            }

            private_mod_interior.push(crate::pointer_constants::node_word_array_declaration(
                ctx,
                "ENCODED_NODE",
                *node_reader,
                crate::pointer_constants::WordArrayDeclarationOptions { pub_crate: true },
            )?);
            private_mod_interior.push(Line(fmt!(ctx,"pub(crate) static ARENA: {capnp}::private::arena::GeneratedCodeArena = {capnp}::private::arena::GeneratedCodeArena::new(&ENCODED_NODE);")));
            method_type_arms.push(Line(fmt!(
                ctx,
                "_ => {capnp}::introspect::panic_invalid_field_index(index),"
            )));
            private_mod_interior.push(Branch(vec![
                line("#[allow(clippy::match_single_binding, clippy::extra_unused_type_parameters)]"),
                Line(fmt!(ctx,"pub(crate) fn get_method_types<{}>(index:u16) -> {capnp}::introspect::MethodTypes {} {{",params.params,params.where_clause)),
                indent(line("match index {")),indent(indent(method_type_arms)),indent(line("}")),line("}")
            ]));
            let mut superclass_arms = Vec::new();
            for (index, parent) in interface.get_superclasses()?.iter().enumerate() {
                let typ = do_branding(
                    ctx,
                    parent.get_id(),
                    parent.get_brand()?,
                    Leaf::Owned,
                    &ctx.get_qualified_module(parent.get_id()),
                )?;
                superclass_arms.push(Line(fmt!(
                    ctx,
                    "{index} => <{typ} as {capnp}::introspect::Introspect>::introspect(),"
                )));
            }
            superclass_arms.push(Line(fmt!(
                ctx,
                "_ => {capnp}::introspect::panic_invalid_field_index(index),"
            )));
            private_mod_interior.push(Branch(vec![
                line("#[allow(clippy::match_single_binding, clippy::extra_unused_type_parameters)]"),
                Line(fmt!(ctx,"pub(crate) fn get_superclass<{}>(index:u16) -> {capnp}::introspect::Type {} {{",params.params,params.where_clause)),
                indent(line("match index {")),indent(indent(superclass_arms)),indent(line("}")),line("}")
            ]));
            let interface_type=fmt!(ctx,"{capnp}::introspect::TypeVariant::Interface({capnp}::introspect::RawBrandedInterfaceSchema {{ arena: &_private::ARENA, method_types: _private::get_method_types::<{}>, superclass: _private::get_superclass::<{}>, brand: {capnp}::introspect::Brand::new({}, _private::get_brand_parameter::<{}>) }})",params.params,params.params,params.expanded_list.len(),params.params);
            client_impl_interior.push(Line(fmt!(ctx,"pub fn schema() -> {capnp}::schema::InterfaceSchema {{ let {capnp}::introspect::TypeVariant::Interface(raw) = <Owned{bracketed_params} as {capnp}::introspect::Introspect>::introspect().which() else {{ unreachable!() }}; raw.into() }}")));

            let mut base_dispatch_arms = Vec::new();
            let mut server_hooks_default = "::core::option::Option::None".to_string();

            let server_base = {
                let mut base_traits = Vec::new();

                fn find_super_interfaces<'a>(
                    interface: schema_capnp::node::interface::Reader<'a>,
                    all_extends: &mut Vec<
                        <schema_capnp::superclass::Owned as capnp::traits::OwnedStruct>::Reader<'a>,
                    >,
                    ctx: &GeneratorContext<'a>,
                    active: &mut HashSet<u64>,
                    budget: &mut usize,
                ) -> ::capnp::Result<()> {
                    // Preserve branded paths through diamonds while bounding cycles,
                    // stack depth and exponential expansion in untrusted requests.
                    if active.len() >= 64 {
                        return Err(Error::failed(
                            "interface inheritance nesting limit exceeded (64)".into(),
                        ));
                    }
                    for superclass in interface.get_superclasses()? {
                        if *budget == 0 {
                            return Err(Error::failed(
                                "interface inheritance expansion limit exceeded (65536)".into(),
                            ));
                        }
                        *budget -= 1;
                        let id = superclass.get_id();
                        if !active.insert(id) {
                            return Err(Error::failed("cyclic interface inheritance".into()));
                        }
                        let parent = ctx
                            .node_map
                            .get(&id)
                            .ok_or_else(|| Error::failed("missing superclass schema".into()))?;
                        let node::Interface(interface) = parent.which()? else {
                            return Err(Error::failed("superclass must be an interface".into()));
                        };
                        find_super_interfaces(interface, all_extends, ctx, active, budget)?;
                        active.remove(&id);
                        all_extends.push(superclass);
                    }
                    Ok(())
                }

                let mut extends = Vec::new();
                find_super_interfaces(
                    interface,
                    &mut extends,
                    ctx,
                    &mut HashSet::from([node_id]),
                    &mut 65_536,
                )?;
                let mut dispatched = HashSet::new();
                for interface in &extends {
                    let type_id = interface.get_id();
                    let brand = interface.get_brand()?;
                    let the_mod = ctx.get_qualified_module(type_id);

                    // Diamond inheritance can reach the same interface more
                    // than once. RPC dispatch has only an interface ID, so keep
                    // the first arm, preserving the existing routing choice.
                    // Retain the branded base-trait obligations below.
                    if dispatched.insert(type_id) {
                        base_dispatch_arms.push(Line(format!(
                            "0x{type_id:x} => {}::dispatch_call_internal(self.server, method_id, params, results),",
                            do_branding(
                                ctx, type_id, brand, Leaf::ServerDispatch, &the_mod)?)));
                    }
                    let base_trait = do_branding(ctx, type_id, brand, Leaf::Server, &the_mod)?;
                    let inherited_hooks =
                        format!("<Self as {base_trait}>::_capnp_server_hooks(self)");
                    server_hooks_default = if base_traits.is_empty() {
                        inherited_hooks
                    } else {
                        format!("{inherited_hooks}.or_else(|| {server_hooks_default})")
                    };
                    base_traits.push(base_trait);
                }

                // Defining that the server itself should always be 'static makes
                // bounds easier down the line.
                if !extends.is_empty() {
                    format!(": {} + 'static", base_traits.join(" + "))
                } else {
                    ": 'static".to_string()
                }
            };

            mod_interior.push(BlankLine);
            mod_interior.push(node_doc.clone());
            mod_interior.push(Line(format!("pub struct Client{bracketed_params} {{")));
            mod_interior.push(indent(Line(fmt!(
                ctx,
                "pub client: {capnp}::capability::Client,"
            ))));
            if is_generic {
                mod_interior.push(indent(Line(params.phantom_data_type.clone())));
            }
            mod_interior.push(line("}"));
            mod_interior.push(
                Branch(vec![
                    Line(fmt!(ctx,"impl {bracketed_params} {capnp}::capability::FromClientHook for Client{bracketed_params} {} {{", params.where_clause)),
                    indent(Line(fmt!(ctx,"fn interface_schema() -> ::core::option::Option<{capnp}::schema::InterfaceSchema> {{ ::core::option::Option::Some(Self::schema()) }}"))),
                    indent(Line(fmt!(ctx,"fn new(hook: Box<{capnp}::capability::DynClientHook>) -> Self {{"))),
                    indent(indent(Line(fmt!(ctx,"Self {{ client: {capnp}::capability::Client::new(hook), {} }}", params.phantom_data_value)))),
                    indent(line("}")),
                    indent(Line(fmt!(ctx,"fn into_client_hook(self) -> Box<{capnp}::capability::DynClientHook> {{"))),
                    indent(indent(line("self.client.hook"))),
                    indent(line("}")),
                    indent(Line(fmt!(ctx,"fn as_client_hook(&self) -> &{capnp}::capability::DynClientHook {{"))),
                    indent(indent(line("&*self.client.hook"))),
                    indent(line("}")),
                    line("}")]));

            mod_interior.push(Line(fmt!(ctx,"impl <{}> From<Client{bracketed_params}> for {capnp}::dynamic_capability::Client {} {{ fn from(client: Client{bracketed_params}) -> Self {{ Self::new(client, <Client{bracketed_params}>::schema()) }} }}",params.params,params.where_clause)));
            mod_interior.push(if !is_generic {
                Branch(vec![
                    Line("#[derive(Copy, Clone)]".into()),
                    line("pub struct Owned(());"),
                    Line(fmt!(ctx,"impl {capnp}::introspect::Introspect for Owned {{ fn introspect() -> {capnp}::introspect::Type {{ {interface_type}.into() }} }}")),
                    Line(fmt!(ctx,"impl {capnp}::traits::Owned for Owned {{ type Reader<'a> = Client; type Builder<'a> = Client; }}")),
                    Line(fmt!(ctx,"impl {capnp}::traits::Pipelined for Owned {{ type Pipeline = Client; }}"))])
            } else {
                Branch(vec![
                    Line("#[derive(Copy, Clone)]".into()),
                    Line(format!("pub struct Owned<{}> {} {{", params.params, params.where_clause)),
                    indent(Line(params.phantom_data_type.clone())),
                    line("}"),
                    Line(fmt!(ctx,
                              "impl <{0}> {capnp}::introspect::Introspect for Owned <{0}> {1} {{ fn introspect() -> {capnp}::introspect::Type {{ {interface_type}.into() }} }}",
                              params.params, params.where_clause)),
                    Line(fmt!(ctx,
                        "impl <{0}> {capnp}::traits::Owned for Owned <{0}> {1} {{ type Reader<'a> = Client<{0}>; type Builder<'a> = Client<{0}>; }}",
                        params.params, params.where_clause)),
                    Line(fmt!(ctx,
                        "impl <{0}> {capnp}::traits::Pipelined for Owned <{0}> {1} {{ type Pipeline = Client{2}; }}",
                        params.params, params.where_clause, bracketed_params))])
            });

            mod_interior.push(Branch(vec![
                Line(fmt!(ctx,"impl <'a,{0}> {capnp}::traits::FromPointerReader<'a> for Client<{0}> {1} {{",
                    params.params, params.where_clause)),
                indent(vec![
                        Line(fmt!(ctx,"fn get_from_pointer(reader: &{capnp}::private::layout::PointerReader<'a>, _default: ::core::option::Option<&'a [{capnp}::Word]>) -> {capnp}::Result<Self> {{")),
                        indent(Line(fmt!(ctx,"::core::result::Result::Ok({capnp}::capability::FromClientHook::new(reader.get_capability()?))"))),
                        line("}")]),
                line("}")]));

            mod_interior.push(Branch(vec![
                Line(fmt!(ctx,"impl <'a,{0}> {capnp}::traits::FromPointerBuilder<'a> for Client<{0}> {1} {{",
                             params.params, params.where_clause)),
                indent(vec![
                            Line(fmt!(ctx,"fn init_pointer(_builder: {capnp}::private::layout::PointerBuilder<'a>, _size: u32) -> Self {{")),
                            indent(line("unimplemented!()")),
                            line("}"),
                            Line(fmt!(ctx,"fn get_from_pointer(builder: {capnp}::private::layout::PointerBuilder<'a>, _default: ::core::option::Option<&'a [{capnp}::Word]>) -> {capnp}::Result<Self> {{")),
                            indent(Line(fmt!(ctx,"::core::result::Result::Ok({capnp}::capability::FromClientHook::new(builder.get_capability()?))"))),
                            line("}")]),
                line("}"),
                BlankLine]));

            mod_interior.push(Branch(vec![
                Line(fmt!(ctx,
                    "impl <{0}> {capnp}::traits::SetterInput<Owned<{0}>> for Client<{0}> {1} {{",
                    params.params, params.where_clause)),
                indent(vec![
                            Line(fmt!(ctx,"fn set_pointer_builder(mut pointer: {capnp}::private::layout::PointerBuilder<'_>, from: Self, _canonicalize: bool) -> {capnp}::Result<()> {{")),
                            indent(Line("pointer.set_capability(from.client.hook);".to_string())),
                            indent(Line("::core::result::Result::Ok(())".to_string())),
                            line("}")
                        ]
                ),
                line("}")]));

            mod_interior.push(Branch(vec![
                Line(fmt!(ctx,
                    "impl {bracketed_params} {capnp}::traits::HasTypeId for Client{bracketed_params} {{"
                )),
                indent(Line(
                    "const TYPE_ID: u64 = _private::TYPE_ID;".to_string(),
                )),
                line("}"),
            ]));

            mod_interior.push(Branch(vec![
                Line(format!(
                    "impl {bracketed_params} Clone for Client{bracketed_params} {{"
                )),
                indent(line("fn clone(&self) -> Self {")),
                indent(indent(Line(format!(
                    "Self {{ client: self.client.clone(), {} }}",
                    params.phantom_data_value
                )))),
                indent(line("}")),
                line("}"),
            ]));

            mod_interior.push(Branch(vec![
                Line(format!(
                    "impl {bracketed_params} Client{bracketed_params} {} {{",
                    params.where_clause
                )),
                indent(client_impl_interior),
                line("}"),
            ]));

            mod_interior.push(Branch(vec![
                node_doc.clone(),
                Line(format!(
                    "pub trait Server<{}> {} {} {{",
                    params.params, server_base, params.where_clause
                )),
                indent(Line(fmt!(ctx, "fn _capnp_server_hooks(&self) -> ::core::option::Option<&dyn {capnp}::capability::ServerHooks> {{ {server_hooks_default} }}"))),
                indent(server_interior),
                line("}"),
            ]));

            mod_interior.push(Branch(vec![
                Line(format!(
                    "pub struct ServerDispatch<_T,{}> {{",
                    params.params
                )),
                indent(line(fmt!(ctx, "pub server: {capnp}::capability::Rc<_T>,"))),
                indent(if is_generic {
                    vec![Line(params.phantom_data_type.clone())]
                } else {
                    vec![]
                }),
                line("}"),
            ]));

            mod_interior.push(Branch(vec![
                Line(
                    fmt!(ctx,"impl <_S: Server{1} + 'static, {0}> {capnp}::capability::FromServer<_S> for Client{1} {2}  {{",
                            params.params, bracketed_params, params.where_clause_with_static)),
                indent(vec![
                    Line(format!("type Dispatch = ServerDispatch<_S, {}>;", params.params)),
                    Line(fmt!(ctx, "fn from_server(s: {capnp}::capability::Rc<_S>) -> ServerDispatch<_S, {}> {{", params.params)),
                    indent(Line(format!("ServerDispatch {{ server: s, {} }}", params.phantom_data_value))),
                    line("}"),
                ]),
                line("}"),
            ]));

            mod_interior.push(
                Branch(vec![
                    (if is_generic {
                        Line(format!("impl <{}, _T: Server{}> ::core::ops::Deref for ServerDispatch<_T,{}> {} {{", params.params, bracketed_params, params.params, params.where_clause))
                    } else {
                        line("impl <_T: Server> ::core::ops::Deref for ServerDispatch<_T> {")
                    }),
                    indent(line("type Target = _T;")),
                    indent(line("fn deref(&self) -> &_T { &self.server}")),
                    line("}"),
                    ]));

            mod_interior.push(
                Branch(vec![
                    (if is_generic {
                        Line(format!("impl <{}, _T: Server{}> ::core::clone::Clone for ServerDispatch<_T,{}> {} {{", params.params, bracketed_params, params.params, params.where_clause))
                    } else {
                        line("impl <_T: Server> ::core::clone::Clone for ServerDispatch<_T> {")
                    }),
                    indent(line(
                        format!("fn clone(&self) -> Self {{ Self {{ server: self.server.clone(), {} }} }}", params.phantom_data_value))),
                    line("}"),
                    ]));

            mod_interior.push(
                Branch(vec![
                    (if is_generic {
                        Line(fmt!(ctx,"impl <{}, _T: Server{}> {capnp}::capability::Server for ServerDispatch<_T,{}> {} {{", params.params, bracketed_params, params.params, params.where_clause_with_static))
                    } else {
                        Line(fmt!(ctx,"impl <_T: Server> {capnp}::capability::Server for ServerDispatch<_T> {{"))
                    }),
                    indent(Line(fmt!(ctx,"fn dispatch_call(self, interface_id: u64, method_id: u16, params: {capnp}::capability::Params<{capnp}::any_pointer::Owned>, results: {capnp}::capability::Results<{capnp}::any_pointer::Owned>) -> {capnp}::capability::DispatchCallResult {{"))),
                    indent(indent(line("match interface_id {"))),
                    indent(indent(indent(line("_private::TYPE_ID => Self::dispatch_call_internal(self.server, method_id, params, results),")))),
                    indent(indent(indent(base_dispatch_arms))),
                    indent(indent(indent(Line(fmt!(ctx,"_ => {{ {capnp}::capability::DispatchCallResult::new({capnp}::capability::Promise::err({capnp}::Error::unimplemented(\"Method not implemented.\".to_string())), false) }}"))))),
                    indent(indent(line("}"))),
                    indent(line("}")),

                    indent(Line(fmt!(ctx, "fn as_ptr(&self) -> usize {{ {capnp}::capability::Rc::as_ptr(&self.server) as usize }}"))),
                    indent(Line(fmt!(ctx, "fn get_hooks(&self) -> ::core::option::Option<&dyn {capnp}::capability::ServerHooks> {{ <_T as Server{bracketed_params}>::_capnp_server_hooks(&*self.server) }}"))),

                    line("}")]));

            mod_interior.push(
                Branch(vec![
                    (if is_generic {
                        Line(format!("impl <{}, _T: Server{}> ServerDispatch<_T,{}> {} {{", params.params, bracketed_params, params.params, params.where_clause_with_static))
                    } else {
                        line("impl <_T :Server> ServerDispatch<_T> {")
                    }),

                    indent(line("#[allow(clippy::match_single_binding)]")),
                    indent(Line(fmt!(ctx,"pub fn dispatch_call_internal(this: {capnp}::capability::Rc<_T>, method_id: u16, params: {capnp}::capability::Params<{capnp}::any_pointer::Owned>, results: {capnp}::capability::Results<{capnp}::any_pointer::Owned>) -> {capnp}::capability::DispatchCallResult {{"))),
                    indent(indent(line("match method_id {"))),
                    indent(indent(indent(dispatch_arms))),
                    indent(indent(indent(Line(fmt!(ctx,"_ => {{ {capnp}::capability::DispatchCallResult::new({capnp}::capability::Promise::err({capnp}::Error::unimplemented(\"Method not implemented.\".to_string())), false) }}"))))),
                    indent(indent(line("}"))),
                    indent(line("}")),
                    line("}")]));

            mod_interior.push(Branch(vec![
                line("pub(crate) mod _private {"),
                indent(private_mod_interior),
                line("}"),
            ]));

            mod_interior.push(Branch(vec![Branch(nested_output)]));

            output.push(BlankLine);
            output.push(node_doc.clone());
            if is_generic {
                output.push(Line(format!(
                    "pub mod {} {{ /* ({}) */",
                    node_name,
                    params.expanded_list.join(",")
                )));
            } else {
                output.push(Line(format!("pub mod {node_name} {{")));
            }
            output.push(indent(mod_interior));
            output.push(line("}"));
        }

        node::Const(c) => {
            let styled_name = snake_to_upper_case(ctx.get_last_name(node_id)?);

            let typ = c.get_type()?;
            let formatted_text = match (typ.which()?, c.get_value()?.which()?) {
                (type_::Void(()), value::Void(())) => {
                    Line(format!("pub const {styled_name}: () = ();"))
                }
                (type_::Bool(()), value::Bool(b)) => {
                    Line(format!("pub const {styled_name}: bool = {b};"))
                }
                (type_::Int8(()), value::Int8(i)) => {
                    Line(format!("pub const {styled_name}: i8 = {i};"))
                }
                (type_::Int16(()), value::Int16(i)) => {
                    Line(format!("pub const {styled_name}: i16 = {i};"))
                }
                (type_::Int32(()), value::Int32(i)) => {
                    Line(format!("pub const {styled_name}: i32 = {i};"))
                }
                (type_::Int64(()), value::Int64(i)) => {
                    Line(format!("pub const {styled_name}: i64 = {i};"))
                }
                (type_::Uint8(()), value::Uint8(i)) => {
                    Line(format!("pub const {styled_name}: u8 = {i};"))
                }
                (type_::Uint16(()), value::Uint16(i)) => {
                    Line(format!("pub const {styled_name}: u16 = {i};"))
                }
                (type_::Uint32(()), value::Uint32(i)) => {
                    Line(format!("pub const {styled_name}: u32 = {i};"))
                }
                (type_::Uint64(()), value::Uint64(i)) => {
                    Line(format!("pub const {styled_name}: u64 = {i};"))
                }

                (type_::Float32(()), value::Float32(f)) => {
                    let literal = match f.classify() {
                        std::num::FpCategory::Nan => "f32::NAN".into(),
                        std::num::FpCategory::Infinite => {
                            if f.is_sign_positive() {
                                "f32::INFINITY".into()
                            } else {
                                "f32::NEG_INFINITY".into()
                            }
                        }
                        _ => format!("{f:e}"),
                    };
                    Line(format!("pub const {styled_name}: f32 = {literal};"))
                }

                (type_::Float64(()), value::Float64(f)) => {
                    let literal = match f.classify() {
                        std::num::FpCategory::Nan => "f64::NAN".into(),
                        std::num::FpCategory::Infinite => {
                            if f.is_sign_positive() {
                                "f64::INFINITY".into()
                            } else {
                                "f64::NEG_INFINITY".into()
                            }
                        }
                        _ => format!("{f:e}"),
                    };
                    Line(format!("pub const {styled_name}: f64 = {literal};"))
                }

                (type_::Enum(e), value::Enum(v)) => {
                    if let Some(node) = ctx.node_map.get(&e.get_type_id()) {
                        match node.which()? {
                            node::Enum(e) => {
                                let enumerants = e.get_enumerants()?;
                                if let Some(enumerant) = enumerants.try_get(u32::from(v)) {
                                    let variant =
                                        capitalize_first_letter(get_enumerant_name(enumerant)?);
                                    let type_string = typ.type_string(ctx, Leaf::Owned)?;
                                    Line(format!(
                                        "pub const {}: {} = {}::{};",
                                        styled_name, type_string, type_string, variant
                                    ))
                                } else {
                                    return Err(Error::failed(format!(
                                        "enumerant out of range: {v}"
                                    )));
                                }
                            }
                            _ => {
                                return Err(Error::failed(format!(
                                    "bad enum type ID: {}",
                                    e.get_type_id()
                                )));
                            }
                        }
                    } else {
                        return Err(Error::failed(format!(
                            "bad enum type ID: {}",
                            e.get_type_id()
                        )));
                    }
                }

                (type_::Text(()), value::Text(t)) => Line(format!(
                    "pub const {styled_name}: &str = {:?};",
                    t?.to_str()?
                )),
                (type_::Data(()), value::Data(d)) => {
                    Line(format!("pub const {styled_name}: &[u8] = &{:?};", d?))
                }

                (type_::List(_), value::List(v)) => {
                    generate_pointer_constant(ctx, &styled_name, typ, v, false)?
                }
                (type_::Struct(_), value::Struct(v)) => {
                    generate_pointer_constant(ctx, &styled_name, typ, v, false)?
                }

                (type_::Interface(_t), value::Interface(())) => {
                    return Err(Error::unimplemented("interface constants".to_string()));
                }
                (type_::AnyPointer(_), value::AnyPointer(v)) => {
                    generate_pointer_constant(ctx, &styled_name, typ, v, false)?
                }

                _ => {
                    return Err(Error::failed("type does not match value".to_string()));
                }
            };

            output.push(documentation::attach(&node_doc, formatted_text));
        }

        node::Annotation(annotation_reader) => {
            let is_generic = node_reader.get_is_generic();
            let params = node_reader.parameters_texts(ctx);
            let last_name = ctx.get_last_name(node_id)?;
            let mut interior = vec![];
            interior.push(Line(format!("pub const ID: u64 = 0x{node_id:x};")));

            let ty = annotation_reader.get_type()?;
            if !is_generic {
                interior.push(Line(fmt!(ctx,
                    "pub fn get_type() -> {capnp}::introspect::Type {{ <{} as {capnp}::introspect::Introspect>::introspect() }}", ty.type_string(ctx, Leaf::Owned)?)));
            } else {
                interior.push(Line(fmt!(ctx,"pub fn get_type<{0}>() -> {capnp}::introspect::Type {1} {{ <{2} as {capnp}::introspect::Introspect>::introspect() }}", params.params, params.where_clause, ty.type_string(ctx, Leaf::Owned)?)));
            }
            output.push(Branch(vec![
                node_doc.clone(),
                Line(format!("pub mod {last_name} {{")),
                indent(interior),
                Line("}".into()),
            ]));
        }
    }

    Ok(Branch(output))
}
