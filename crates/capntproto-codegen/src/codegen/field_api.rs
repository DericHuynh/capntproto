//! Opt-in static facade; wire metadata, RPC dispatch and reflection continue to
//! come from the established generator in the same file.
mod values;
use super::*;
use capnp::schema_capnp::{field, node, type_, value};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
fn title(s: &str) -> String {
    s.split('_')
        .filter(|p| !p.is_empty())
        .map(|p| {
            let mut c = p.chars();
            c.next().unwrap().to_uppercase().collect::<String>() + c.as_str()
        })
        .collect()
}
fn union_variant(s: &str) -> String {
    let name = ident(&title(s));
    if name == "Unknown" {
        "Unknown_".into()
    } else {
        name
    }
}
fn ident(s: &str) -> String {
    match s {
        "self" | "Self" | "super" | "crate" => format!("{s}_"),
        "type" | "match" | "ref" | "mut" | "move" | "loop" | "in" | "fn" | "pub" | "use"
        | "mod" | "struct" | "enum" | "const" | "static" | "trait" | "impl" | "where" | "as"
        | "async" | "await" | "dyn" | "return" | "break" | "continue" | "if" | "else" | "let"
        | "for" | "while" | "unsafe" | "extern" | "true" | "false" | "box" | "yield" | "try"
        | "gen" => format!("r#{s}"),
        _ => s.into(),
    }
}
fn diagnostic_location(
    ctx: &GeneratorContext,
    id: u64,
    field: field::Reader<'_>,
) -> capnp::Result<String> {
    let schema = ctx.node_map[&id].get_display_name()?.to_str()?;
    let name = field.get_name()?.to_str()?;
    let ordinal = match field.get_ordinal().which()? {
        field::ordinal::Explicit(n) => format!("Some({n})"),
        field::ordinal::Implicit(()) => "None".into(),
    };
    Ok(format!("{}::field_api::diagnostics::Location::Field{{schema:{schema:?},name:{name:?},ordinal:{ordinal}}}", ctx.capnp_root))
}
pub(super) fn path(ctx: &GeneratorContext, id: u64) -> capnp::Result<String> {
    let mut p = ctx.scope_map[&id].clone();
    let mut root = id;
    while !matches!(ctx.node_map[&root].which()?, node::File(())) {
        root = ctx.node_parents[&root];
    }
    let at = ctx.scope_map[&root].len();
    p.insert(at, "api".into());
    let last = p.pop().unwrap();
    p.push(title(&last));
    Ok(p.join("::"))
}
pub(super) fn remap(ctx: &GeneratorContext, mut text: String) -> capnp::Result<String> {
    let mut replacements = Vec::new();
    for (&id, n) in &ctx.node_map {
        if ctx.scope_map.contains_key(&id) {
            if let node::Struct(_) = n.which()? {
                replacements.push((
                    format!("{}::Owned", ctx.scope_map[&id].join("::")),
                    path(ctx, id)?,
                ));
            }
        }
    }
    replacements.sort_by_key(|(a, _)| std::cmp::Reverse(a.len()));
    for (a, b) in replacements {
        text = text.replace(&a, &b);
    }
    Ok(text)
}
fn kind(ctx: &GeneratorContext, t: type_::Reader<'_>) -> capnp::Result<String> {
    let f = format!("{}::field_api", ctx.capnp_root);
    Ok(match t.which()? {
        type_::Text(()) => format!("{f}::Text"),
        type_::Data(()) => format!("{f}::Data"),
        type_::Struct(_) => format!(
            "{f}::Struct<{}>",
            remap(ctx, t.type_string(ctx, Leaf::Owned)?)?
        ),
        type_::List(l) => format!("{f}::List<{}>", element(ctx, l.get_element_type()?)?),
        type_::Interface(_) => format!(
            "{f}::Capability<{}>",
            remap(ctx, t.type_string(ctx, Leaf::Client)?)?
        ),
        type_::AnyPointer(_) => format!(
            "{f}::Generic<{}>",
            remap(ctx, t.type_string(ctx, Leaf::Owned)?)?
        ),
        _ => return Err(Error::failed("expected pointer kind".into())),
    })
}
fn element(ctx: &GeneratorContext, t: type_::Reader<'_>) -> capnp::Result<String> {
    Ok(match t.which()? {
        type_::Struct(_) => remap(ctx, t.type_string(ctx, Leaf::Owned)?)?,
        type_::Enum(e) => path(ctx, e.get_type_id())?,
        _ if t.is_prim()? || matches!(t.which()?, type_::Enum(_)) => {
            t.type_string(ctx, Leaf::Owned)?
        }
        _ => kind(ctx, t)?,
    })
}
fn scalar_type(ctx: &GeneratorContext, t: type_::Reader<'_>) -> capnp::Result<String> {
    match t.which()? {
        type_::Enum(e) => path(ctx, e.get_type_id()),
        _ => t.type_string(ctx, Leaf::Owned),
    }
}
fn mask(v: value::Reader<'_>) -> capnp::Result<String> {
    Ok(match v.which()? {
        value::Void(()) => "()".into(),
        value::Bool(v) => v.to_string(),
        value::Int8(v) => v.to_string(),
        value::Int16(v) => v.to_string(),
        value::Int32(v) => v.to_string(),
        value::Int64(v) => v.to_string(),
        value::Uint8(v) => v.to_string(),
        value::Uint16(v) => v.to_string(),
        value::Uint32(v) => v.to_string(),
        value::Uint64(v) => v.to_string(),
        value::Float32(v) => v.to_bits().to_string(),
        value::Float64(v) => v.to_bits().to_string(),
        value::Enum(v) => v.to_string(),
        _ => return Err(Error::failed("not a scalar default".into())),
    })
}
pub(super) fn add_method_scopes(ctx: &mut GeneratorContext) -> capnp::Result<()> {
    let nodes = ctx.node_map.values().copied().collect::<Vec<_>>();
    for n in nodes {
        if let node::Interface(i) = n.which()? {
            let Some(scope) = ctx.scope_map.get(&n.get_id()).cloned() else {
                continue;
            };
            for m in i.get_methods()? {
                for (id, suffix) in [
                    (m.get_param_struct_type(), "Params"),
                    (m.get_result_struct_type(), "Results"),
                ] {
                    if ctx.node_map[&id].get_scope_id() == 0 {
                        ctx.populate_scope_map(
                            scope.clone(),
                            format!("{}{suffix}", m.get_name()?.to_str()?),
                            NameKind::Module,
                            id,
                        )?;
                    }
                }
            }
        }
    }
    Ok(())
}
// Clear only schema-known storage; recursively include union group payloads.
fn clear_field(
    ctx: &GeneratorContext,
    field: field::Reader<'_>,
    out: &mut String,
) -> capnp::Result<()> {
    match field.which()? {
        field::Group(g) => {
            let node::Struct(s) = ctx.node_map[&g.get_type_id()].which()? else {
                unreachable!()
            };
            if s.get_discriminant_count() > 0 {
                writeln!(
                    out,
                    "b.set_data_field::<u16>({},0);",
                    s.get_discriminant_offset()
                )
                .unwrap();
            }
            for f in s.get_fields()? {
                clear_field(ctx, f, out)?;
            }
        }
        field::Slot(slot) => {
            let t = slot.get_type()?;
            let o = slot.get_offset();
            match t.which()? {
                type_::Void(()) => (),
                type_::Bool(()) => {
                    writeln!(out, "b.set_bool_field({o},false);").unwrap();
                }
                _ if t.is_prim()? || matches!(t.which()?, type_::Enum(_)) => {
                    let typ = if matches!(t.which()?, type_::Enum(_)) {
                        "u16".into()
                    } else {
                        t.type_string(ctx, Leaf::Owned)?
                    };
                    writeln!(out, "b.set_data_field::<{typ}>({o},0_{typ});").unwrap();
                }
                _ => {
                    writeln!(
                        out,
                        "if keep!=Some({o}){{b.reborrow().get_pointer_field({o}).clear();}}"
                    )
                    .unwrap();
                }
            }
        }
    }
    Ok(())
}
// Union arms can overlap and bools can share words with parent siblings. Merge
// physical masks instead of copying whole parent words or moving a slot twice.
fn group_storage(
    ctx: &GeneratorContext,
    id: u64,
    data: &mut BTreeMap<usize, u64>,
    pointers: &mut BTreeSet<usize>,
) -> capnp::Result<()> {
    let node::Struct(s) = ctx.node_map[&id].which()? else {
        unreachable!()
    };
    fn bits(data: &mut BTreeMap<usize, u64>, offset: usize, count: usize) {
        for bit in offset..offset + count {
            *data.entry(bit / 64).or_default() |= 1u64 << (bit % 64);
        }
    }
    if s.get_discriminant_count() > 0 {
        bits(data, s.get_discriminant_offset() as usize * 16, 16);
    }
    for field in s.get_fields()? {
        match field.which()? {
            field::Group(g) => group_storage(ctx, g.get_type_id(), data, pointers)?,
            field::Slot(slot) => {
                let width = match slot.get_type()?.which()? {
                    type_::Void(()) => 0,
                    type_::Bool(()) => 1,
                    type_::Int8(()) | type_::Uint8(()) => 8,
                    type_::Int16(()) | type_::Uint16(()) | type_::Enum(_) => 16,
                    type_::Int32(()) | type_::Uint32(()) | type_::Float32(()) => 32,
                    type_::Int64(()) | type_::Uint64(()) | type_::Float64(()) => 64,
                    _ => {
                        pointers.insert(slot.get_offset() as usize);
                        continue;
                    }
                };
                bits(data, slot.get_offset() as usize * width, width);
            }
        }
    }
    Ok(())
}
pub(super) fn generate(ctx: &GeneratorContext, id: u64) -> capnp::Result<String> {
    let mut out=String::from("\n/// Borrowed readers and field-operation editors (opt-in generated API).\n#[allow(dead_code, unused_imports, clippy::extra_unused_type_parameters, clippy::should_implement_trait)]\npub mod api {\n");
    emit_children(ctx, id, &mut out)?;
    out.push_str("}\n");
    Ok(out)
}
fn emit_children(ctx: &GeneratorContext, id: u64, out: &mut String) -> capnp::Result<()> {
    let n = ctx.node_map[&id];
    for child in n.get_nested_nodes()? {
        emit(ctx, child.get_id(), out)?;
    }
    if let node::Struct(s) = n.which()? {
        for f in s.get_fields()? {
            if let field::Group(g) = f.which()? {
                emit(ctx, g.get_type_id(), out)?;
            }
        }
    }
    if let node::Interface(i) = n.which()? {
        for m in i.get_methods()? {
            for id in [m.get_param_struct_type(), m.get_result_struct_type()] {
                if ctx.node_map[&id].get_scope_id() == 0 {
                    emit(ctx, id, out)?;
                }
            }
        }
    }
    Ok(())
}
fn emit(ctx: &GeneratorContext, id: u64, out: &mut String) -> capnp::Result<()> {
    let n = ctx.node_map[&id];
    let name = title(ctx.get_last_name(id)?);
    let cp = &ctx.capnp_root;
    let f = format!("{cp}::field_api");
    let l = format!("{cp}::private::layout");
    let compiled = ctx.scope_map[&id].join("::");
    let node_doc = ctx.documentation.attribute(id, None);
    match n.which()? {
        node::Enum(e) => {
            out.push_str(node_doc);
            writeln!(
                out,
                "#[derive(Clone, Copy, Debug, PartialEq, Eq)] pub enum {name} {{"
            )
            .unwrap();
            for (i, v) in e.get_enumerants()?.iter().enumerate() {
                out.push_str(ctx.documentation.attribute(id, Some(i)));
                let vn = ident(&title(get_enumerant_name(v)?));
                writeln!(out, "{},", if vn == "Unknown" { "Unknown_" } else { &vn }).unwrap();
            }
            out.push_str("Unknown(u16),}\n");
            // Open enums share the native enum's schema identity, including the
            // erased enclosing generic scopes. The raw ordinal stays lossless.
            writeln!(out,"impl {cp}::introspect::Introspect for {name} {{fn introspect()->{cp}::introspect::Type{{<{compiled} as {cp}::introspect::Introspect>::introspect()}}}}").unwrap();
            writeln!(out,"impl From<{name}> for {cp}::dynamic_value::Reader<'_> {{fn from(value:{name})->Self{{let {cp}::introspect::TypeVariant::Enum(raw)=<{name} as {cp}::introspect::Introspect>::introspect().which() else {{unreachable!()}};{cp}::dynamic_value::Enum::new(value.into(),raw.into()).into()}}}}").unwrap();
            writeln!(
                out,
                "impl From<u16> for {name} {{ fn from(value:u16)->Self{{match value{{"
            )
            .unwrap();
            for (i, v) in e.get_enumerants()?.iter().enumerate() {
                let vn = ident(&title(get_enumerant_name(v)?));
                writeln!(
                    out,
                    "{i}=>Self::{},",
                    if vn == "Unknown" { "Unknown_" } else { &vn }
                )
                .unwrap();
            }
            out.push_str("n=>Self::Unknown(n),}}}\n");
            writeln!(
                out,
                "impl From<{name}> for u16 {{fn from(value:{name})->Self{{match value{{"
            )
            .unwrap();
            for (i, v) in e.get_enumerants()?.iter().enumerate() {
                let vn = ident(&title(get_enumerant_name(v)?));
                writeln!(
                    out,
                    "{name}::{}=>{i},",
                    if vn == "Unknown" { "Unknown_" } else { &vn }
                )
                .unwrap();
            }
            writeln!(out, "{name}::Unknown(n)=>n,}}}}}}").unwrap();
            writeln!(out,"impl {f}::Scalar for {name} {{type Mask=u16;const BITS:usize=16;fn read(r: {l}::StructReader<'_>,o:usize,m:u16)->Self{{r.get_data_field_mask::<u16>(o,m).into()}}fn write(r:&{l}::StructBuilder<'_>,o:usize,m:u16,v:Self){{r.set_data_field_mask(o,u16::from(v),m);}}}}").unwrap();
            writeln!(out,"impl {f}::Element for {name} {{type Ref<'a>=Self;type Mut<'a>={f}::EnumElement<'a,Self>;const SIZE: {l}::ElementSize={l}::ElementSize::TwoBytes;fn get<'a>(r: {l}::ListReader<'a>,i:u32)->Self{{<u16 as {l}::PrimitiveElement>::get(&r,i).into()}}fn get_mut<'a>(r: {l}::ListBuilder<'a>,i:u32)->Self::Mut<'a>{{{f}::EnumElement::new(r,i)}}}}").unwrap();
        }
        node::Struct(s) => {
            let p = n.parameters_texts(ctx);
            let ps = &p.params;
            let wc = &p.where_clause;
            let generic = if ps.is_empty() {
                String::new()
            } else {
                format!("{ps},")
            };
            let marker = if ps.is_empty() {
                name.clone()
            } else {
                format!("{name}<{ps}>")
            };
            let old = if ps.is_empty() {
                format!("{compiled}::Owned")
            } else {
                format!("{compiled}::Owned<{ps}>")
            };
            let phantom = if ps.is_empty() {
                "()".to_string()
            } else {
                format!("({ps},)")
            };
            let args = |mode: &str| format!("{name}<{generic}{mode}>");
            let read = args(&format!("{f}::mode::Read<'a>"));
            let write = args(&format!("{f}::mode::Write<'a>"));
            let refname = format!("{name}Ref");
            let mutname = format!("{name}Mut");
            writeln!(out,"{node_doc}pub struct {name}<{generic}M: {f}::Mode={f}::mode::Schema>(M,::core::marker::PhantomData<{phantom}>) {wc};\n{node_doc}pub type {refname}<'a,{ps}> = {read};\n{node_doc}pub type {mutname}<'a,{ps}> = {write};").unwrap();
            writeln!(out,"impl<{generic}M: {f}::Mode+Copy> Copy for {name}<{generic}M> {wc} {{}}\nimpl<{generic}M: {f}::Mode+Copy> Clone for {name}<{generic}M> {wc} {{fn clone(&self)->Self{{*self}}}}").unwrap();
            let display_name = n.get_display_name()?.to_str()?;
            writeln!(out,"impl<{ps}> {f}::Schema for {marker} {wc} {{const NAME: &'static str={display_name:?};type Ref<'a>={read};type Mut<'a>={write};const SIZE: {l}::StructSize={l}::StructSize{{data:{},pointers:{}}};}}",s.get_data_word_count(),s.get_pointer_count()).unwrap();
            if s.get_is_group() {
                let mut data = BTreeMap::new();
                let mut pointers = BTreeSet::new();
                group_storage(ctx, id, &mut data, &mut pointers)?;
                let data: Vec<_> = data.into_iter().collect();
                let pointers: Vec<_> = pointers.into_iter().collect();
                writeln!(out,"impl<{ps}> {f}::GroupSchema for {marker} {wc} {{const DATA_MASKS:&'static [(usize,u64)]=&{data:?};const POINTERS:&'static [usize]=&{pointers:?};}}").unwrap();
            }
            writeln!(out,"impl<'a,{ps}> From<{l}::StructReader<'a>> for {read} {wc} {{fn from(raw: {l}::StructReader<'a>)->Self{{Self({f}::mode::Read::new(raw),::core::marker::PhantomData)}}}}\nimpl<'a,{ps}> From<{l}::StructBuilder<'a>> for {write} {wc} {{fn from(raw: {l}::StructBuilder<'a>)->Self{{{f}::check_layout(&raw,<{marker} as {f}::Schema>::SIZE);Self({f}::mode::Write::new(raw),::core::marker::PhantomData)}}}}").unwrap();
            writeln!(out,"impl<'a,{ps}> {cp}::traits::IntoInternalStructReader<'a> for {read} {wc} {{fn into_internal_struct_reader(self)->{l}::StructReader<'a>{{self.0.raw()}}}}\nimpl<{ps}> {cp}::introspect::Introspect for {marker} {wc} {{fn introspect()->{cp}::introspect::Type{{<{old} as {cp}::introspect::Introspect>::introspect()}}}}").unwrap();
            writeln!(out,"impl<{ps}> {cp}::traits::Owned for {marker} {wc} {{type Reader<'a>={read};type Builder<'a>={write};}}\nimpl<'a,{ps}> {cp}::traits::FromPointerReader<'a> for {read} {wc} {{fn get_from_pointer(p:&{l}::PointerReader<'a>,d:Option<&'a[{cp}::Word]>)->{cp}::Result<Self>{{Ok(p.get_struct(d)?.into())}}}}\nimpl<'a,{ps}> {cp}::traits::FromPointerBuilder<'a> for {write} {wc} {{fn init_pointer(p: {l}::PointerBuilder<'a>,_:u32)->Self{{p.init_struct(<{marker} as {f}::Schema>::SIZE).into()}}fn get_from_pointer(p: {l}::PointerBuilder<'a>,d:Option<&'a[{cp}::Word]>)->{cp}::Result<Self>{{Ok(p.get_struct(<{marker} as {f}::Schema>::SIZE,d)?.into())}}}}\nimpl<'a,{ps}> {cp}::traits::SetterInput<{marker}> for {read} {wc} {{fn set_pointer_builder(mut p: {l}::PointerBuilder<'_>,v:Self,c:bool)->{cp}::Result<()>{{p.set_struct(&v.0.raw(),c)}}}}").unwrap();
            writeln!(out,"impl<{ps}> {cp}::traits::OwnedStruct for {marker} {wc} {{type Reader<'a>={read};type Builder<'a>={write};}}\nimpl<'a,{ps}> {cp}::traits::HasStructSize for {write} {wc} {{const STRUCT_SIZE: {l}::StructSize=<{marker} as {f}::Schema>::SIZE;}}").unwrap();
            writeln!(out,"impl<{ps}> {cp}::traits::Pipelined for {marker} {wc} {{type Pipeline=<{old} as {cp}::traits::Pipelined>::Pipeline;}}").unwrap();
            writeln!(out,"impl<{ps}> {f}::Element for {marker} {wc} {{type Ref<'a>={read};type Mut<'a>={write};const SIZE: {l}::ElementSize={l}::ElementSize::InlineComposite;const STRUCT:Option<{l}::StructSize>=Some(<Self as {f}::Schema>::SIZE);fn get<'a>(r: {l}::ListReader<'a>,i:u32)->Self::Ref<'a>{{r.get_struct_element(i).into()}}fn get_mut<'a>(r: {l}::ListBuilder<'a>,i:u32)->Self::Mut<'a>{{r.get_struct_element(i).into()}}}}").unwrap();
            let mut readers = String::new();
            let mut writers = String::new();
            let mut descriptors = String::new();
            let mut variants = Vec::new();
            let mut union_docs = BTreeMap::new();
            for (index, field) in s.get_fields()?.iter().enumerate() {
                let fname = get_field_name(field)?;
                let schema_name = field.get_name()?.to_str()?;
                let location = diagnostic_location(ctx, id, field)?;
                let ordinal = match field.get_ordinal().which()? {
                    field::ordinal::Explicit(n) => n,
                    field::ordinal::Implicit(()) => index as u16,
                };
                let snake = camel_to_snake_case(fname);
                let method = ident(&snake);
                if ["read", "field", "which", "tag", "into_raw", "copy_from"]
                    .contains(&snake.as_str())
                {
                    return Err(Error::failed(format!("field API reserved method {snake} on {name}; use $Rust.name to rename the field")));
                }
                let disc = field.get_discriminant_value();
                let union = disc != field::NO_DISCRIMINANT;
                let selection = if union {
                    format!(
                        "Some({f}::Selection{{offset:{},tag:{disc},clear:Self::__clear_union}})",
                        s.get_discriminant_offset()
                    )
                } else {
                    "None".into()
                };
                let guard = if union {
                    format!("if self.0.raw().get_data_field::<u16>({})!={disc}{{return Err({location}.error({cp}::Error::from_kind({cp}::ErrorKind::NotPresent)));}}",s.get_discriminant_offset())
                } else {
                    String::new()
                };
                if union {
                    union_docs.insert(disc, ctx.documentation.attribute(id, Some(index)));
                }
                let doc = format!(
                    "{}/// Schema field `{schema_name}` (ordinal {ordinal}).\n",
                    ctx.documentation.attribute(id, Some(index))
                );
                match field.which()? {
                    field::Slot(slot) => {
                        let t = slot.get_type()?;
                        let off = slot.get_offset();
                        if t.is_prim()? || matches!(t.which()?, type_::Enum(_)) {
                            let typ = scalar_type(ctx, t)?;
                            let mask = mask(slot.get_default_value()?)?;
                            let void = matches!(t.which()?, type_::Void(()));
                            let expr = if void {
                                "()".into()
                            } else {
                                format!("<{typ} as {f}::Scalar>::read(self.0.raw(),{off},{mask})")
                            };
                            let ret = if union {
                                format!("{cp}::Result<{typ}>")
                            } else {
                                typ.clone()
                            };
                            let expr = if union { format!("Ok({expr})") } else { expr };
                            writeln!(
                                readers,
                                "{doc}#[inline] pub fn {method}(&self)->{ret}{{{guard}{expr}}}"
                            )
                            .unwrap();
                            let handle = if void {
                                format!("{f}::VoidField")
                            } else {
                                format!("{f}::ScalarField")
                            };
                            let ht = if void {
                                format!("{handle}<'_>")
                            } else {
                                format!("{handle}<'_,{typ}>")
                            };
                            let call = if void {
                                format!("{handle}::new(self.0.raw(),{selection})")
                            } else {
                                format!("{handle}::new(self.0.raw(),{off},{mask},{selection})")
                            };
                            writeln!(
                                writers,
                                "{doc}#[inline] pub fn {method}(&mut self)->{ht}{{{call}}}"
                            )
                            .unwrap();
                            if union {
                                variants.push((
                                    disc,
                                    union_variant(&snake),
                                    typ,
                                    format!("self.{method}()?"),
                                    void,
                                ));
                            }
                        } else {
                            let k = kind(ctx, t)?;
                            let def = if slot.get_had_explicit_default() {
                                let dn = format!(
                                    "__DEFAULT_{}_{}",
                                    name.to_uppercase(),
                                    snake.to_uppercase()
                                );
                                let v = slot.get_default_value()?;
                                out.push_str(&stringify(
                                    &crate::pointer_constants::word_array_declaration(
                                        ctx,
                                        &dn,
                                        capnp::raw::get_struct_pointer_section(v).get(0),
                                        crate::pointer_constants::WordArrayDeclarationOptions {
                                            pub_crate: false,
                                        },
                                    )?,
                                ));
                                format!("Some(&{dn})")
                            } else {
                                "None".into()
                            };
                            let typ = format!("<{k} as {f}::PointerType>::Ref<'a>");
                            let optional = is_option_field(field)?;
                            let ret = if optional {
                                format!("Option<{typ}>")
                            } else {
                                typ.clone()
                            };
                            let expression=format!("<{k} as {f}::PointerType>::read(self.0.raw().get_pointer_field({off}),{def})");
                            let expression = if optional {
                                format!("if self.0.raw().is_pointer_field_null({off}){{Ok(None)}}else{{{expression}.map(Some)}}")
                            } else {
                                expression
                            };
                            writeln!(readers,"{doc}#[inline] pub fn {method}(&self)->{cp}::Result<{ret}>{{{guard}{location}.run(||{{{expression}}})}}").unwrap();
                            writeln!(writers,"{doc}#[inline] pub fn {method}(&mut self)->{f}::PointerField<'_,{k}>{{{f}::PointerField::new(self.0.raw(),{off},{def},{selection}).with_location({location})}}\n{doc}#[inline] pub fn into_{snake}(self)->{f}::PointerField<'a,{k}>{{{f}::PointerField::new(self.0.into_raw(),{off},{def},{selection}).with_location({location})}}").unwrap();
                            let descriptor_selection = if union {
                                format!("Some({f}::Selection{{offset:{},tag:{disc},clear:<{mutname}<'static,{ps}>>::__clear_union}})",s.get_discriminant_offset())
                            } else {
                                "None".into()
                            };
                            writeln!(descriptors,"{doc}pub const {}: {f}::Field<Self,{k}>={f}::Field::new({off},{def},{schema_name:?},{ordinal},{descriptor_selection}).with_schema({display_name:?});",snake.to_uppercase()).unwrap();
                            if union {
                                variants.push((
                                    disc,
                                    union_variant(&snake),
                                    ret,
                                    format!("self.{method}()?"),
                                    false,
                                ));
                            }
                        }
                    }
                    field::Group(g) => {
                        let gp = path(ctx, g.get_type_id())?;
                        let node::Struct(gs) = ctx.node_map[&g.get_type_id()].which()? else {
                            unreachable!()
                        };
                        let pure_union = gs.get_discriminant_count() > 0
                            && gs
                                .get_fields()?
                                .iter()
                                .all(|f| f.get_discriminant_value() != field::NO_DISCRIMINANT);
                        let gt = if ps.is_empty() {
                            gp.clone()
                        } else {
                            format!("{gp}<{ps}>")
                        };
                        let rt = format!("<{gt} as {f}::Schema>::Ref<'a>");
                        let mt = format!("<{gt} as {f}::Schema>::Mut<'_>");
                        let expr =
                            format!("<{rt} as From<{l}::StructReader<'a>>>::from(self.0.raw())");
                        if pure_union && !union {
                            let union_path = gp + "UnionRef";
                            let ut = format!("{union_path}<'a,{ps}>");
                            writeln!(readers,"{doc}pub fn {method}(&self)->{cp}::Result<{ut}>{{{guard}{location}.run(||{expr}.which())}}\n{doc}pub fn {snake}_tag(&self)->{}UnionTag{{{expr}.tag()}}",path(ctx,g.get_type_id())?).unwrap();
                        } else {
                            let ret = if union {
                                format!("{cp}::Result<{rt}>")
                            } else {
                                rt.clone()
                            };
                            let expr = if union { format!("Ok({expr})") } else { expr };
                            writeln!(
                                readers,
                                "{doc}pub fn {method}(&self)->{ret}{{{guard}{expr}}}"
                            )
                            .unwrap();
                        }
                        if union {
                            variants.push((
                                disc,
                                union_variant(&snake),
                                rt,
                                format!("self.{method}()?"),
                                false,
                            ));
                            let selection = format!(
                                "{f}::Selection{{offset:{},tag:{disc},clear:Self::__clear_union}}",
                                s.get_discriminant_offset()
                            );
                            writeln!(writers,"{doc}pub fn {method}(&mut self)->{f}::GroupField<'_,{gt}>{{{f}::GroupField::new(self.0.raw(),{selection}).with_location({location})}}\n{doc}pub fn into_{snake}(self)->{f}::GroupField<'a,{gt}>{{{f}::GroupField::new(self.0.into_raw(),{selection}).with_location({location})}}").unwrap();
                            continue;
                        }
                        writeln!(writers,"{doc}pub fn {method}(&mut self)->{mt}{{self.0.raw().into()}}\n{doc}pub fn into_{snake}(self)-><{gt} as {f}::Schema>::Mut<'a>{{self.0.into_raw().into()}}").unwrap();
                    }
                }
            }
            writeln!(out,"impl<{ps}> {marker} {wc} {{{descriptors}}}\nimpl<'a,{ps}> {read} {wc} {{ {readers}\npub fn field<K: {f}::PointerType>(&self,d: {f}::Field<{marker},K>)->{f}::Inspection<'a,K>{{d.inspect(self.0.raw())}} ").unwrap();
            if !variants.is_empty() {
                let tag = format!("{name}UnionTag");
                let which = format!("{name}UnionRef");
                writeln!(out,"pub fn tag(&self)->{tag}{{self.0.raw().get_data_field::<u16>({}).into()}}\npub fn which(&self)->{cp}::Result<{which}<'a,{ps}>>{{Ok(match self.0.raw().get_data_field::<u16>({}){{",s.get_discriminant_offset(),s.get_discriminant_offset()).unwrap();
                for (d, v, _, expr, void) in &variants {
                    writeln!(
                        out,
                        "{d}=>{which}::{v}{},",
                        if *void {
                            String::new()
                        } else {
                            format!("({expr})")
                        }
                    )
                    .unwrap();
                }
                writeln!(out, "n=>{which}::Unknown(n),}})}}").unwrap();
            }
            out.push_str("}\n");
            writeln!(out,"impl<'a,{ps}> {write} {wc} {{{writers}\npub fn read(&self)->{}{{self.0.read().into()}}",args(&format!("{f}::mode::Read<'_>"))).unwrap();
            if !s.get_is_group() {
                writeln!(out,"pub fn copy_from(&mut self, source: {refname}<'_,{ps}>)->{cp}::Result<()>{{self.0.raw().copy_content_from_strict(&{cp}::traits::IntoInternalStructReader::into_internal_struct_reader(source))}}").unwrap();
            }
            if !variants.is_empty() {
                writeln!(
                    out,
                    "fn __clear_union(b:&mut {l}::StructBuilder<'_>,keep:Option<usize>){{let _=keep;"
                )
                .unwrap();
                for field in s.get_fields()? {
                    if field.get_discriminant_value() != field::NO_DISCRIMINANT {
                        clear_field(ctx, field, out)?;
                    }
                }
                out.push_str("}\n");
            }
            out.push_str("}\n");
            if !variants.is_empty() {
                let tag = format!("{name}UnionTag");
                let which = format!("{name}UnionRef");
                writeln!(
                    out,
                    "#[derive(Clone,Copy,Debug,PartialEq,Eq)] pub enum {tag}{{"
                )
                .unwrap();
                for (d, v, _, _, _) in &variants {
                    out.push_str(union_docs[d]);
                    writeln!(out, "{v},").unwrap();
                }
                out.push_str("Unknown(u16),}\n");
                writeln!(
                    out,
                    "impl From<u16> for {tag}{{fn from(n:u16)->Self{{match n{{"
                )
                .unwrap();
                for (d, v, _, _, _) in &variants {
                    writeln!(out, "{d}=>Self::{v},").unwrap();
                }
                out.push_str("n=>Self::Unknown(n),}}}\n");
                // Phantom variant keeps lifetimes/parameters meaningful for scalar-only unions.
                writeln!(out, "pub enum {which}<'a,{ps}> {wc} {{").unwrap();
                for (d, v, t, _, void) in &variants {
                    out.push_str(union_docs[d]);
                    writeln!(
                        out,
                        "{v}{},",
                        if *void {
                            String::new()
                        } else {
                            format!("({t})")
                        }
                    )
                    .unwrap();
                }
                writeln!(out,"Unknown(u16),#[doc(hidden)] __Lifetime(::core::marker::PhantomData<(&'a (),{ps})>),}}").unwrap();
            }
            if !n.get_nested_nodes()?.is_empty()
                || s.get_fields()?
                    .iter()
                    .any(|f| matches!(f.which(), Ok(field::Group(_))))
            {
                writeln!(out, "pub mod {} {{", ctx.get_last_name(id)?).unwrap();
                emit_children(ctx, id, out)?;
                out.push_str("}\n");
            }
        }
        node::Interface(_) => {
            out.push_str(node_doc);
            writeln!(out, "pub mod {} {{", ctx.get_last_name(id)?).unwrap();
            emit_children(ctx, id, out)?;
            out.push_str("}\n");
        }
        _ => (),
    }
    if ctx.field_api_values || ctx.field_api_projections {
        values::emit(ctx, id, out)?;
    }
    Ok(())
}
