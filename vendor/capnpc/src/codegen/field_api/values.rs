use super::*;

pub(super) fn emit(ctx: &GeneratorContext, id: u64, out: &mut String) -> capnp::Result<()> {
    let n = ctx.node_map[&id];
    let cp = &ctx.capnp_root;
    let f = format!("{cp}::field_api");
    let nv = format!("{f}::native");
    let l = format!("{cp}::private::layout");
    let name = title(ctx.get_last_name(id)?);
    if let node::Enum(_) = n.which()? {
        if ctx.field_api_values {
            writeln!(out, "impl {nv}::NativeElement for {name} {{type Value=Self;fn read_value(raw: {l}::ListReader<'_>,i:u32,_:&mut {nv}::Conversion)->{cp}::Result<Self>{{Ok(<u16 as {l}::PrimitiveElement>::get(&raw,i).into())}}fn write_value(raw: {l}::ListBuilder<'_>,i:u32,v:&Self,_:&mut {nv}::Conversion)->{cp}::Result<()>{{<u16 as {l}::PrimitiveElement>::set(&raw,i,(*v).into());Ok(())}}}}").unwrap();
        }
        return Ok(());
    }
    let node::Struct(s) = n.which()? else {
        return Ok(());
    };
    let p = n.parameters_texts(ctx);
    let ps = &p.params;
    let wc = &p.where_clause;
    let marker = format!("{name}<{ps}>");
    let phantom = if ps.is_empty() {
        "()".to_string()
    } else {
        format!("({ps},)")
    };
    let value = format!("{name}Value<{ps}>");
    let view = format!("{name}View<'a,{ps}>");
    let mut projection_fields = String::new();
    let mut projection_init = String::new();
    let mut value_fields = String::new();
    let mut value_init = String::new();
    let mut value_writes = String::new();
    let mut variants = String::new();
    let mut variant_reads = String::new();
    let mut variant_writes = String::new();
    let union_type = format!("{name}UnionValue");
    for (index, field) in s.get_fields()?.iter().enumerate() {
        let doc = ctx.documentation.attribute(id, Some(index));
        let snake = camel_to_snake_case(get_field_name(field)?);
        let location = diagnostic_location(ctx, id, field)?;
        let method = ident(&snake);
        if (ctx.field_api_projections
            && ["project", "__schema", "__union"].contains(&snake.as_str()))
            || (ctx.field_api_values
                && ["to_value", "__schema", "__union"].contains(&snake.as_str()))
        {
            return Err(Error::failed(format!("field API reserved method/field {snake} on {name}; use $Rust.name to rename the field")));
        }
        let disc = field.get_discriminant_value();
        let union = disc != field::NO_DISCRIMINANT;
        let (proj_type, proj_expr, native_type, native_expr, native_write, void) = match field
            .which()?
        {
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
                        format!("<{typ} as {f}::Scalar>::read(raw,{off},{mask})")
                    };
                    let write = if void {
                        String::new()
                    } else {
                        format!("<{typ} as {f}::Scalar>::write(&raw,{off},{mask},*v);")
                    };
                    (
                        typ.clone(),
                        format!("self.{method}()"),
                        typ,
                        expr,
                        write,
                        void,
                    )
                } else {
                    let k = kind(ctx, t)?;
                    let rt = format!("<{k} as {f}::PointerType>::Ref<'a>");
                    let rt = if is_option_field(field)? {
                        format!("Option<{rt}>")
                    } else {
                        rt
                    };
                    (rt, format!("self.{method}()?"), format!("Option<<{k} as {nv}::NativePointer>::Value>"),
                     format!("{nv}::read_optional::<{k}>(raw.get_pointer_field({off}),b)?"),
                     format!("{nv}::write_optional::<{k}>(raw.reborrow().get_pointer_field({off}),v,b)?;"), false)
                }
            }
            field::Group(g) => {
                let gp = path(ctx, g.get_type_id())?;
                let gt = format!("{gp}<{ps}>");
                let node::Struct(gs) = ctx.node_map[&g.get_type_id()].which()? else {
                    unreachable!()
                };
                let pure_union = gs.get_discriminant_count() > 0
                    && gs
                        .get_fields()?
                        .iter()
                        .all(|f| f.get_discriminant_value() != field::NO_DISCRIMINANT);
                let rt = if pure_union && !union {
                    format!("{gp}UnionRef<'a,{ps}>")
                } else {
                    format!("<{gt} as {f}::Schema>::Ref<'a>")
                };
                let expr = format!(
                    "self.{method}(){}",
                    if pure_union && !union { "?" } else { "" }
                );
                (
                    rt,
                    expr,
                    format!("<{gt} as {nv}::NativeSchema>::Value"),
                    format!("<{gt} as {nv}::NativeSchema>::read_value(raw,b)?"),
                    format!("<{gt} as {nv}::NativeSchema>::write_value(raw.reborrow(),v,b)?;"),
                    false,
                )
            }
        };
        // Preserve the underlying error kind/metadata and allocate context only on failure.
        let native_expr = match native_expr.strip_suffix('?') {
            Some(expr) => format!("{expr}.map_err(|e|{location}.error(e))?"),
            None => native_expr,
        };
        let native_write = match native_write.strip_suffix("?;") {
            Some(expr) => format!("{expr}.map_err(|e|{location}.error(e))?;"),
            None => native_write,
        };
        if !union {
            writeln!(projection_fields, "{doc}pub {method}: {proj_type},").unwrap();
            writeln!(projection_init, "{method}: {proj_expr},").unwrap();
            writeln!(value_fields, "{doc}pub {method}: {native_type},").unwrap();
            writeln!(value_init, "{method}: {native_expr},").unwrap();
            if !void {
                writeln!(value_writes, "{{let v=&value.{method};{native_write}}}").unwrap();
            }
        } else {
            let variant = union_variant(&snake);
            if void {
                writeln!(variants, "{doc}{variant},").unwrap();
                writeln!(variant_reads, "{disc}=>{union_type}::{variant},").unwrap();
                writeln!(
                    variant_writes,
                    "{union_type}::{variant}=>{{raw.set_data_field::<u16>({},{disc});}},",
                    s.get_discriminant_offset()
                )
                .unwrap();
            } else {
                writeln!(variants, "{doc}{variant}({native_type}),").unwrap();
                writeln!(
                    variant_reads,
                    "{disc}=>{union_type}::{variant}({native_expr}),"
                )
                .unwrap();
                writeln!(variant_writes, "{union_type}::{variant}(v)=>{{{native_write}raw.set_data_field::<u16>({},{disc});}},",s.get_discriminant_offset()).unwrap();
            }
        }
    }
    if s.get_discriminant_count() > 0 {
        projection_fields.push_str(&format!("pub __union: {name}UnionRef<'a,{ps}>,\n"));
        projection_init.push_str("__union:self.which()?,\n");
        writeln!(value_fields, "pub __union: {union_type}<{ps}>,").unwrap();
        writeln!(value_init, "__union:match raw.get_data_field::<u16>({}){{{variant_reads}n=>return Err({f}::diagnostics::Location::Union(<{marker} as {f}::Schema>::NAME).error({cp}::Error::from_kind({cp}::ErrorKind::EnumValueOrUnionDiscriminantNotPresent({cp}::NotInSchema(n))))),}},",s.get_discriminant_offset()).unwrap();
        writeln!(value_writes, "match &value.__union {{{variant_writes} {} }}",if ps.is_empty(){String::new()}else{format!("{union_type}::__Schema(_)=>return Err({f}::diagnostics::Location::Union(<{marker} as {f}::Schema>::NAME).error({cp}::Error::failed(\"invalid native union marker\".into()))),")}).unwrap();
    }
    if ctx.field_api_projections {
        out.push_str(ctx.documentation.attribute(id, None));
        writeln!(out, "/// Borrowed decoded fields; nested payloads remain lazy and fallible.\npub struct {view} {wc} {{{projection_fields}__schema: ::core::marker::PhantomData<(&'a (),{ps})>,}}\nimpl<'a,{ps}> {name}Ref<'a,{ps}> {wc} {{pub fn project(&self)->{cp}::Result<{view}>{{Ok({name}View{{{projection_init}__schema: ::core::marker::PhantomData}})}}}}").unwrap();
    }
    if ctx.field_api_values {
        out.push_str(ctx.documentation.attribute(id, None));
        writeln!(out, "/// Allocated known fields. Pointer `None` preserves physical absence and schema defaults.\npub struct {value} {wc} {{{value_fields}__schema: ::core::marker::PhantomData<{phantom}>,}}").unwrap();
        if s.get_discriminant_count() > 0 {
            writeln!(
                out,
                "pub enum {union_type}<{ps}> {wc} {{{variants}{}}}",
                if ps.is_empty() {
                    String::new()
                } else {
                    format!("#[doc(hidden)] __Schema(::core::marker::PhantomData<{phantom}>),")
                }
            )
            .unwrap();
        }
        writeln!(out,"#[allow(unused_variables,unused_mut)] impl<{ps}> {nv}::NativeSchema for {marker} {wc} {{type Value={value};fn read_value(raw: {l}::StructReader<'_>,b:&mut {nv}::Conversion)->{cp}::Result<Self::Value>{{b.nested(|b|{{b.items(1)?;Ok({name}Value{{{value_init}__schema: ::core::marker::PhantomData}})}})}}fn write_value(mut raw: {l}::StructBuilder<'_>,value:&Self::Value,b:&mut {nv}::Conversion)->{cp}::Result<()>{{b.nested(|b|{{b.items(1)?;{value_writes}Ok(())}})}}}}").unwrap();
        writeln!(out,"impl<{ps}> {nv}::NativeElement for {marker} {wc} {{type Value={value};fn read_value(raw: {l}::ListReader<'_>,i:u32,b:&mut {nv}::Conversion)->{cp}::Result<Self::Value>{{<Self as {nv}::NativeSchema>::read_value(raw.get_struct_element(i),b)}}fn write_value(raw: {l}::ListBuilder<'_>,i:u32,v:&Self::Value,b:&mut {nv}::Conversion)->{cp}::Result<()>{{<Self as {nv}::NativeSchema>::write_value(raw.get_struct_element(i),v,b)}}}}").unwrap();
        writeln!(out,"impl<'a,{ps}> {name}Ref<'a,{ps}> {wc} {{pub fn to_value(&self,_policy: {nv}::UnknownFields,limits: {nv}::Limits)->{cp}::Result<{value}>{{<{marker} as {nv}::NativeSchema>::read_value(self.0.raw(),&mut {nv}::Conversion::new(limits)).map_err(|e|{f}::diagnostics::conversion(e,<{marker} as {f}::Schema>::NAME,\"decode\"))}}}}").unwrap();
        if !s.get_is_group() {
            writeln!(out,"impl<{ps}> {value} {wc} {{pub fn new()->{cp}::Result<Self>{{{f}::Message::<{marker}>::new()?.read().to_value({nv}::UnknownFields::Discard,{nv}::Limits::default())}}pub fn to_message(&self,limits: {nv}::Limits)->{cp}::Result<{f}::Message<{marker}>>{{{nv}::to_message::<{marker}>(self,limits)}}}}").unwrap();
        }
    }
    Ok(())
}
