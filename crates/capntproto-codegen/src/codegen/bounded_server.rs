//! Stack-owned async service adapters for the bounded wire API.
use super::*;

pub(super) struct Method {
    pub name: String,
    pub constant: String,
    pub params: String,
    pub results: Option<String>,
    pub cancellation: bool,
    pub no_pipelining: bool,
}

pub(super) fn generate(
    ctx: &GeneratorContext<'_>,
    node: schema_capnp::node::Reader<'_>,
    interface: schema_capnp::node::interface::Reader<'_>,
    methods: Vec<Method>,
) -> ::capnp::Result<FormattedText> {
    // Validate even untrusted CodeGeneratorRequests before emitting recursive
    // parent dispatch. Branded diamonds retain their direct trait obligations.
    fn validate(
        ctx: &GeneratorContext<'_>,
        id: u64,
        active: &mut HashSet<u64>,
        budget: &mut usize,
    ) -> ::capnp::Result<()> {
        if active.len() >= 64 || *budget == 0 {
            return Err(Error::failed(
                "bounded interface inheritance limit exceeded".into(),
            ));
        }
        *budget -= 1;
        if !active.insert(id) {
            return Err(Error::failed("cyclic interface inheritance".into()));
        }
        let parent = ctx
            .node_map
            .get(&id)
            .ok_or_else(|| Error::failed("missing superclass schema".into()))?;
        let schema_capnp::node::Interface(interface) = parent.which()? else {
            return Err(Error::failed("superclass must be an interface".into()));
        };
        for parent in interface.get_superclasses()? {
            validate(ctx, parent.get_id(), active, budget)?;
        }
        active.remove(&id);
        Ok(())
    }
    validate(ctx, node.get_id(), &mut HashSet::new(), &mut 65_536)?;
    Ok(Branch(vec![
        adapter(ctx, node, interface, &methods, false)?,
        adapter(ctx, node, interface, &methods, true)?,
    ]))
}

fn adapter(
    ctx: &GeneratorContext<'_>,
    node: schema_capnp::node::Reader<'_>,
    interface: schema_capnp::node::interface::Reader<'_>,
    methods: &[Method],
    contextual: bool,
) -> ::capnp::Result<FormattedText> {
    let params = node.parameters_texts(ctx);
    let bracketed = if params.params.is_empty() {
        String::new()
    } else {
        format!("<{}>", params.params)
    };
    let owner = format!("Owned{bracketed}");
    let (server, dispatcher, service, dispatch_method) = if contextual {
        (
            "ContextServer",
            "ContextServerDispatch",
            "ContextService<_Context>",
            "dispatch_with_context",
        )
    } else {
        ("Server", "ServerDispatch", "Service", "dispatch")
    };
    let generics = if contextual {
        format!("_Context, {}", params.params)
    } else {
        params.params.clone()
    };
    let server_type = if contextual {
        format!("{server}<{generics}>")
    } else {
        format!("{server}{bracketed}")
    };
    let marker = if contextual {
        format!("({owner}, fn(&mut _Context))")
    } else {
        owner.clone()
    };
    let trait_context = if contextual {
        ", _context: &'call mut _Context"
    } else {
        ""
    };
    let context_arg = if contextual {
        ", context: &'call mut _Context"
    } else {
        ""
    };
    let context_expr = if contextual { ", context" } else { "" };
    let mut bases = Vec::new();
    let mut lookups = Vec::new();
    let mut forwards = Vec::new();
    for parent in interface.get_superclasses()? {
        let module = ctx.get_qualified_module(parent.get_id());
        bases.push(do_branding(
            ctx,
            parent.get_id(),
            parent.get_brand()?,
            if contextual {
                Leaf::ContextServer
            } else {
                Leaf::Server
            },
            &module,
        )?);
        let adapter = do_branding(
            ctx,
            parent.get_id(),
            parent.get_brand()?,
            if contextual {
                Leaf::ContextServerDispatch
            } else {
                Leaf::ServerDispatch
            },
            &module,
        )?;
        // Leaf::ServerDispatch uses _T for the concrete server, as hosted code does.
        lookups.push(line(format!("if let Some(info) = {adapter}::lookup(interface_id, method_id) {{ return Some(info); }}")));
        forwards.push(line(format!("if {adapter}::lookup(interface_id, method_id).is_some() {{ return {adapter}::dispatch_internal(server, interface_id, method_id, params, results{context_expr}).await; }}")));
    }
    let base = if bases.is_empty() {
        String::new()
    } else {
        format!(": {}", bases.join(" + "))
    };
    let mut trait_methods = Vec::new();
    let mut metadata = Vec::new();
    let mut dispatch = Vec::new();
    for (ordinal, method) in methods.iter().enumerate() {
        let Method {
            name,
            constant,
            params: p,
            results,
            cancellation,
            no_pipelining,
        } = method;
        let result_arg = results.as_ref().map_or(String::new(), |r| {
            fmt!(
                ctx,
                ", _results: {capnp}::field_api::FixedStruct<'call, {r}>"
            )
        });
        trait_methods.push(line(fmt!(ctx, "fn {name}<'call>(&'call mut self, _params: <{p} as {capnp}::field_api::Schema>::Ref<'call>{result_arg}{trait_context}) -> impl ::core::future::Future<Output = {capnp}::Result<()>> + 'call {{ ::core::future::ready(Err({capnp}::Error::from_kind({capnp}::ErrorKind::Unimplemented))) }}")));
        let streaming = results.is_none();
        metadata.push(line(fmt!(ctx, "{ordinal} => Some({capnp}::wire_capability::MethodInfo {{ allow_cancellation: {cancellation}, no_promise_pipelining: {no_pipelining}, streaming: {streaming} }}),")));
        let result_expr = results.as_ref().map_or(String::new(), |r| {
            fmt!(
                ctx,
                ", results.cast::<{capnp}::field_api::Struct<{r}>>().init()?"
            )
        });
        dispatch.push(line(format!("{ordinal} => <_T as {server_type}>::{name}(server, <{owner}>::{constant}.read_params(params)?{result_expr}{context_expr}).await,")));
    }
    metadata.push(line("_ => None,"));
    dispatch.push(line(fmt!(
        ctx,
        "_ => Err({capnp}::Error::from_kind({capnp}::ErrorKind::Unimplemented)),"
    )));
    let result_field = fmt!(ctx, "{capnp}::field_api::FixedField<'call, {capnp}::field_api::Generic<{capnp}::any_pointer::Owned>>");
    Ok(Branch(vec![
        line("/// Borrowed async methods; futures and result buffers stay caller-owned."),
        line(format!("pub trait {server}<{generics}>{base} {} {{", params.where_clause)), indent(trait_methods), line("}"),
        line(format!("pub struct {dispatcher}<_T, {generics}> {} {{ pub server: _T, marker: ::core::marker::PhantomData<{marker}> }}", params.where_clause)),
        line(format!("impl<_T: {server_type}, {generics}> {dispatcher}<_T, {generics}> {} {{", params.where_clause)),
        indent(vec![
            line("pub const fn new(server: _T) -> Self { Self { server, marker: ::core::marker::PhantomData } }"),
            line(fmt!(ctx, "pub fn lookup(interface_id: u64, method_id: u16) -> Option<{capnp}::wire_capability::MethodInfo> {{")),
            indent(vec![line("if interface_id == _private::TYPE_ID { return match method_id {"), indent(metadata), line("}; }"), Branch(lookups), line("None")]), line("}"),
            line(fmt!(ctx, "pub async fn dispatch_internal<'call>(server: &'call mut _T, interface_id: u64, method_id: u16, params: {capnp}::any_pointer::Reader<'call>, results: {result_field}{context_arg}) -> {capnp}::Result<()> {{")),
            indent(vec![line("if interface_id == _private::TYPE_ID { return match method_id {"), indent(dispatch), line("}; }"), Branch(forwards), line(fmt!(ctx, "Err({capnp}::Error::from_kind({capnp}::ErrorKind::Unimplemented))"))]), line("}"),
        ]), line("}"),
        line(fmt!(ctx, "impl<_T: {server_type}, {generics}> {capnp}::wire_capability::{service} for {dispatcher}<_T, {generics}> {} {{", params.where_clause)),
        indent(vec![
            line(fmt!(ctx, "fn method_info(&self, interface_id: u64, method_id: u16) -> Option<{capnp}::wire_capability::MethodInfo> {{ Self::lookup(interface_id, method_id) }}")),
            line(fmt!(ctx, "async fn {dispatch_method}<'call>(&'call mut self, interface_id: u64, method_id: u16, params: {capnp}::any_pointer::Reader<'call>, results: {result_field}{context_arg}) -> {capnp}::Result<()> {{ Self::dispatch_internal(&mut self.server, interface_id, method_id, params, results{context_expr}).await }}")),
        ]), line("}"),
    ]))
}
