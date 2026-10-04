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

mod documentation;
mod field_api;
mod node;
use node::generate_node;

use std::collections;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use capnp::schema_capnp;
use capnp::Error;

use self::FormattedText::{BlankLine, Branch, Indent, Line};
use crate::codegen_types::{do_branding, Leaf, RustNodeInfo, RustTypeInfo, TypeParameterTexts};
use crate::convert_io_err;
use crate::pointer_constants::generate_pointer_constant;

/// An invocation of the capnpc-rust code generation plugin.
pub struct CodeGenerationCommand {
    output_directory: PathBuf,
    default_parent_module: Vec<String>,
    raw_code_generator_request_path: Option<PathBuf>,
    capnp_root: String,
    crates_provide_map: HashMap<u64, String>,
    field_api: bool,
    field_api_values: bool,
    field_api_projections: bool,
    structured_replies: bool,
}

impl Default for CodeGenerationCommand {
    fn default() -> Self {
        Self {
            output_directory: PathBuf::new(),
            default_parent_module: Vec::new(),
            raw_code_generator_request_path: None,
            capnp_root: "::capnp".into(),
            crates_provide_map: HashMap::new(),
            field_api: false,
            field_api_values: false,
            field_api_projections: false,
            structured_replies: false,
        }
    }
}

impl CodeGenerationCommand {
    /// Creates a new code generation command with default options.
    pub fn new() -> Self {
        Self::default()
    }

    /// Also emit the API-design reader/editor facade under each file's `api` module.
    pub fn field_api(&mut self, enabled: bool) -> &mut Self {
        self.field_api = enabled || self.field_api_values || self.field_api_projections;
        self
    }

    /// Generate allocating native values. Also enables the field API facade.
    pub fn field_api_values(&mut self, enabled: bool) -> &mut Self {
        self.field_api_values = enabled;
        if enabled {
            self.field_api = true;
        }
        self
    }

    /// Generate borrowed field projections. Also enables the field API facade.
    pub fn field_api_projections(&mut self, enabled: bool) -> &mut Self {
        self.field_api_projections = enabled;
        if enabled {
            self.field_api = true;
        }
        self
    }

    /// Generate server Results aliases as affine `capability::Reply` contexts.
    /// Choosing editing or tail forwarding consumes the fresh context; publishing
    /// consumes and freezes the editor. This changes Rust server signatures only,
    /// not the wire schema, clients, dispatch or cancellation policy.
    pub fn structured_replies(&mut self, enabled: bool) -> &mut Self {
        self.structured_replies = enabled;
        self
    }

    /// Sets the output directory.
    pub fn output_directory<P>(&mut self, path: P) -> &mut Self
    where
        P: AsRef<Path>,
    {
        self.output_directory = path.as_ref().to_path_buf();
        self
    }

    /// Sets the default parent module, indicating the scope in your crate where you will
    /// add the generated code.
    ///
    /// This option can be overridden by the `parentModule` annotation defined in `rust.capnp`.
    pub fn default_parent_module(&mut self, default_parent_module: Vec<String>) -> &mut Self {
        self.default_parent_module = default_parent_module;
        self
    }

    /// Sets the runtime path used by generated code. Defaults to `::capnp`.
    ///
    /// Use `::runtime` for a renamed dependency, `crate::wire` for a module that
    /// re-exports the runtime, or `crate` when bootstrapping schema.capnp inside
    /// the runtime. Build scripts can use [`crate::CompilerCommand::capnp_root`].
    pub fn capnp_root(&mut self, capnp_root: &str) -> &mut Self {
        self.capnp_root = capnp_root.into();
        self
    }

    /// Sets the raw code generator request output path.
    pub fn raw_code_generator_request_path<P>(&mut self, path: P) -> &mut Self
    where
        P: AsRef<Path>,
    {
        self.raw_code_generator_request_path = Some(path.as_ref().to_path_buf());
        self
    }

    /// Sets the crate provides map.
    ///
    /// # Arguments
    ///
    /// - `map` - A map from capnp file id to the crate name that provides the
    ///   corresponding generated code.
    ///
    /// See [`crate::CompilerCommand::crate_provides`] for more details.
    pub fn crates_provide_map(&mut self, map: HashMap<u64, String>) -> &mut Self {
        self.crates_provide_map = map;
        self
    }

    /// Generates Rust code according to a `schema_capnp::code_generator_request` read from `inp`.
    pub fn run<T>(&mut self, inp: T) -> ::capnp::Result<()>
    where
        T: std::io::Read,
    {
        use capnp::serialize;
        use std::io::Write;

        let message = serialize::read_message(inp, capnp::message::ReaderOptions::new())?;

        let mut ctx = GeneratorContext::new_from_code_generation_command(self, &message)?;
        if self.field_api {
            field_api::add_method_scopes(&mut ctx)?;
        }

        for requested_file in ctx.request.get_requested_files()? {
            let id = requested_file.get_id();
            let mut filepath = self.output_directory.to_path_buf();
            let requested = ::std::path::PathBuf::from(requested_file.get_filename()?.to_str()?);
            filepath.push(requested);
            if let Some(parent) = filepath.parent() {
                ::std::fs::create_dir_all(parent).map_err(convert_io_err)?;
            }

            let root_name = path_to_stem_string(&filepath)?.replace('-', "_");
            filepath.set_file_name(format!("{root_name}_capnp.rs"));

            let capnp_version = match ctx.request.get_capnp_version() {
                Ok(version) => format!(
                    "{}.{}.{}",
                    version.get_major(),
                    version.get_minor(),
                    version.get_micro()
                ),
                Err(_) => "<unknown>".to_owned(),
            };

            let lines =
                Branch(vec![
                Line(
                    "// @generated by the capnpc-rust plugin to the Cap'n Proto schema compiler."
                        .to_string(),
                ),
                line("// DO NOT EDIT."),
                Line(format!("// source: {}", requested_file.get_filename()?.to_str()?)),
                Line(format!("// capnp binary version: {}", capnp_version)),
                Line(format!("// capnpc crate version: {}", env!("CARGO_PKG_VERSION"))),
                BlankLine,
                generate_node(&ctx, id, &root_name)?,
            ]);

            let mut text = stringify(&lines);
            if self.field_api {
                text.push_str(&field_api::generate(&ctx, id)?);
            }

            let previous_text = ::std::fs::read(&filepath);
            if previous_text.is_ok() && previous_text.unwrap() == text.as_bytes() {
                // File is unchanged. Do not write it so that builds with the
                // output as part of the source work in read-only filesystems
                // and so timestamp-based build systems and watchers do not get
                // confused.
                continue;
            }

            // It would be simpler to use the ? operator instead of a pattern match, but then the error message
            // would not include `filepath`.
            match ::std::fs::File::create(&filepath) {
                Ok(mut writer) => {
                    writer.write_all(text.as_bytes()).map_err(convert_io_err)?;
                }
                Err(e) => {
                    let _ = writeln!(
                        &mut ::std::io::stderr(),
                        "could not open file {filepath:?} for writing: {e}"
                    );
                    return Err(convert_io_err(e));
                }
            }
        }

        if let Some(raw_code_generator_request) = &self.raw_code_generator_request_path {
            let raw_code_generator_request_file =
                ::std::fs::File::create(raw_code_generator_request).map_err(convert_io_err)?;
            serialize::write_message_segments(
                raw_code_generator_request_file,
                &message.into_segments(),
            )?;
        }

        Ok(())
    }
}

pub struct GeneratorContext<'a> {
    pub request: schema_capnp::code_generator_request::Reader<'a>,
    pub node_map: collections::hash_map::HashMap<u64, schema_capnp::node::Reader<'a>>,
    pub scope_map: collections::hash_map::HashMap<u64, Vec<String>>,

    /// Map from node ID to the node ID of its parent scope. This is equal to node.scope_id
    /// for all nodes except for autogenerated interface Param and Result structs;
    /// those have scope_id set to 0. See the comment on paramStructType in schema.capnp.
    pub node_parents: collections::hash_map::HashMap<u64, u64>,

    /// Root path for referencing things in the `capnp` crate from the generated code.
    pub capnp_root: String,
    documentation: documentation::Documentation,
    field_api: bool,
    field_api_values: bool,
    field_api_projections: bool,
    structured_replies: bool,
}

impl<'a> GeneratorContext<'a> {
    pub fn new(
        message: &'a capnp::message::Reader<capnp::serialize::OwnedSegments>,
    ) -> ::capnp::Result<GeneratorContext<'a>> {
        GeneratorContext::new_from_code_generation_command(&Default::default(), message)
    }

    fn new_from_code_generation_command(
        code_generation_command: &CodeGenerationCommand,
        message: &'a capnp::message::Reader<capnp::serialize::OwnedSegments>,
    ) -> ::capnp::Result<GeneratorContext<'a>> {
        let mut default_parent_module_scope = vec!["crate".to_string()];
        default_parent_module_scope
            .extend_from_slice(&code_generation_command.default_parent_module[..]);

        let mut ctx = GeneratorContext {
            request: message.get_root()?,
            node_map: collections::hash_map::HashMap::<u64, schema_capnp::node::Reader<'a>>::new(),
            scope_map: collections::hash_map::HashMap::<u64, Vec<String>>::new(),
            node_parents: collections::hash_map::HashMap::new(),
            capnp_root: code_generation_command.capnp_root.clone(),
            documentation: documentation::Documentation::new(message.get_root()?)?,
            field_api: code_generation_command.field_api,
            field_api_values: code_generation_command.field_api_values,
            field_api_projections: code_generation_command.field_api_projections,
            structured_replies: code_generation_command.structured_replies,
        };

        let crates_provide = &code_generation_command.crates_provide_map;

        for node in ctx.request.get_nodes()? {
            ctx.node_map.insert(node.get_id(), node);
            ctx.node_parents.insert(node.get_id(), node.get_scope_id());
        }

        // Fix up "anonymous" method params and results scopes.
        for node in ctx.request.get_nodes()? {
            if let Ok(schema_capnp::node::Interface(interface_reader)) = node.which() {
                for method in interface_reader.get_methods()? {
                    let param_struct_type = method.get_param_struct_type();
                    if ctx.node_parents.get(&param_struct_type) == Some(&0) {
                        ctx.node_parents.insert(param_struct_type, node.get_id());
                    }
                    let result_struct_type = method.get_result_struct_type();
                    if ctx.node_parents.get(&result_struct_type) == Some(&0) {
                        ctx.node_parents.insert(result_struct_type, node.get_id());
                    }
                }
            }
        }

        for requested_file in ctx.request.get_requested_files()? {
            let id = requested_file.get_id();

            for import in requested_file.get_imports()? {
                let importpath = ::std::path::Path::new(import.get_name()?.to_str()?);
                let root_name: String = format!(
                    "{}_capnp",
                    path_to_stem_string(importpath)?.replace('-', "_")
                );
                let parent_module_scope = if let Some(krate) = crates_provide.get(&import.get_id())
                {
                    vec![format!("::{krate}")]
                } else {
                    default_parent_module_scope.clone()
                };

                ctx.populate_scope_map(
                    parent_module_scope,
                    root_name,
                    NameKind::Verbatim,
                    import.get_id(),
                )?;
            }

            let root_name = path_to_stem_string(requested_file.get_filename()?.to_str()?)?;
            let root_mod = format!("{}_capnp", root_name.replace('-', "_"));
            ctx.populate_scope_map(
                default_parent_module_scope.clone(),
                root_mod,
                NameKind::Verbatim,
                id,
            )?;
        }
        Ok(ctx)
    }

    fn get_last_name(&self, id: u64) -> ::capnp::Result<&str> {
        match self.scope_map.get(&id) {
            None => Err(Error::failed(format!("node not found: {id}"))),
            Some(v) => match v.last() {
                None => Err(Error::failed(format!("node has no scope: {id}"))),
                Some(n) => Ok(n),
            },
        }
    }

    fn populate_scope_map(
        &mut self,
        mut ancestor_scope_names: Vec<String>,
        mut current_node_name: String,
        current_name_kind: NameKind,
        node_id: u64,
    ) -> ::capnp::Result<()> {
        // unused nodes in imported files might be omitted from the node map
        let Some(&node_reader) = self.node_map.get(&node_id) else {
            return Ok(());
        };

        for annotation in node_reader.get_annotations()? {
            if annotation.get_id() == NAME_ANNOTATION_ID {
                current_node_name = name_annotation_value(annotation)?.to_string();
            } else if annotation.get_id() == PARENT_MODULE_ANNOTATION_ID {
                let head = ancestor_scope_names[0].clone();
                ancestor_scope_names.clear();
                ancestor_scope_names.push(head);
                ancestor_scope_names.append(&mut get_parent_module(annotation)?);
            }
        }

        let mut scope_names = ancestor_scope_names;
        scope_names.push(capnp_name_to_rust_name(
            &current_node_name,
            current_name_kind,
        ));

        self.scope_map.insert(node_id, scope_names.clone());

        let nested_nodes = node_reader.get_nested_nodes()?;
        for nested_node in nested_nodes {
            let nested_node_id = nested_node.get_id();
            match self.node_map.get(&nested_node_id) {
                None => {}
                Some(node_reader) => match node_reader.which() {
                    Ok(schema_capnp::node::Enum(_enum_reader)) => {
                        self.populate_scope_map(
                            scope_names.clone(),
                            nested_node.get_name()?.to_string()?,
                            NameKind::Verbatim,
                            nested_node_id,
                        )?;
                    }
                    _ => {
                        self.populate_scope_map(
                            scope_names.clone(),
                            nested_node.get_name()?.to_string()?,
                            NameKind::Module,
                            nested_node_id,
                        )?;
                    }
                },
            }
        }

        if let Ok(schema_capnp::node::Struct(struct_reader)) = node_reader.which() {
            let fields = struct_reader.get_fields()?;
            for field in fields {
                if let Ok(schema_capnp::field::Group(group)) = field.which() {
                    self.populate_scope_map(
                        scope_names.clone(),
                        get_field_name(field)?.to_string(),
                        NameKind::Module,
                        group.get_type_id(),
                    )?;
                }
            }
        }

        Ok(())
    }

    pub fn get_qualified_module(&self, type_id: u64) -> String {
        self.scope_map[&type_id].join("::")
    }
}

/// Like `format!(...)`, but adds a `capnp=ctx.capnp_root` argument.
macro_rules! fmt(
    ($ctx:ident, $($arg:tt)*) => ( format!($($arg)*, capnp=$ctx.capnp_root) )
);

pub(crate) use fmt;

fn path_to_stem_string<P: AsRef<::std::path::Path>>(path: P) -> ::capnp::Result<String> {
    if let Some(stem) = path.as_ref().file_stem() {
        stem.to_owned()
            .into_string()
            .map_err(|os_string| Error::failed(format!("bad filename: {os_string:?}")))
    } else {
        Err(Error::failed(format!(
            "file has no stem: {:?}",
            path.as_ref()
        )))
    }
}

fn snake_to_upper_case(s: &str) -> String {
    let mut result_chars: Vec<char> = Vec::new();
    for c in s.chars() {
        if c == '_' {
            result_chars.push('_');
        } else {
            result_chars.push(c.to_ascii_uppercase());
        }
    }
    result_chars.into_iter().collect()
}

fn camel_to_snake_case(s: &str) -> String {
    let mut result_chars: Vec<char> = Vec::new();
    let mut first_char = true;
    for c in s.chars() {
        if c.is_uppercase() && !first_char {
            result_chars.push('_');
        }
        result_chars.push(c.to_ascii_lowercase());
        first_char = false;
    }
    result_chars.into_iter().collect()
}

fn capitalize_first_letter(s: &str) -> String {
    let mut result_chars: Vec<char> = Vec::new();
    for c in s.chars() {
        result_chars.push(c)
    }
    result_chars[0] = result_chars[0].to_ascii_uppercase();
    let result: String = result_chars.into_iter().collect();
    if result == "Self" {
        "Self_".into()
    } else {
        result
    }
}

/// Formats a u64 into a string representation of the hex value, with
/// separating underscores. Used instead of simple hex formatting to prevent
/// clippy warnings in autogenerated code. This is loosely based off of
/// similar functionality in the `separator` crate.
fn format_u64(value: u64) -> String {
    let hex = format!("{value:#x}");
    let mut separated = hex[0..2].to_string();
    let mut place = hex.len() - 2;
    let mut later_loop = false;

    for ch in hex[2..].chars() {
        if later_loop && place % 4 == 0 {
            separated.push('_');
        }

        separated.push(ch);
        later_loop = true;
        place -= 1;
    }

    separated
}

#[test]
fn test_camel_to_snake_case() {
    assert_eq!(camel_to_snake_case("fooBar"), "foo_bar".to_string());
    assert_eq!(camel_to_snake_case("FooBar"), "foo_bar".to_string());
    assert_eq!(camel_to_snake_case("fooBarBaz"), "foo_bar_baz".to_string());
    assert_eq!(camel_to_snake_case("FooBarBaz"), "foo_bar_baz".to_string());
    assert_eq!(camel_to_snake_case("helloWorld"), "hello_world".to_string());
    assert_eq!(camel_to_snake_case("HelloWorld"), "hello_world".to_string());
    assert_eq!(camel_to_snake_case("uint32Id"), "uint32_id".to_string());

    assert_eq!(camel_to_snake_case("fooBar_"), "foo_bar_".to_string());
}

#[derive(PartialEq, Clone)]
pub enum FormattedText {
    Indent(Box<FormattedText>),
    Branch(Vec<FormattedText>),
    Line(String),
    BlankLine,
}

impl From<Vec<FormattedText>> for FormattedText {
    fn from(value: Vec<FormattedText>) -> Self {
        Branch(value)
    }
}

pub fn indent(inner: impl Into<FormattedText>) -> FormattedText {
    Indent(Box::new(inner.into()))
}

pub fn line(inner: impl ToString) -> FormattedText {
    Line(inner.to_string())
}

fn to_lines(ft: &FormattedText, indent: usize) -> Vec<String> {
    match ft {
        Indent(ft) => to_lines(ft, indent + 1),
        Branch(fts) => fts.iter().flat_map(|ft| to_lines(ft, indent)).collect(),
        Line(s) => {
            let mut s1: String = " ".repeat(indent * 4);
            s1.push_str(s);
            vec![s1.to_string()]
        }
        BlankLine => vec!["".to_string()],
    }
}

fn stringify(ft: &FormattedText) -> String {
    let mut result = to_lines(ft, 0).join("\n");
    result.push('\n');
    result
}

const RUST_KEYWORDS: &[&str] = &[
    "abstract", "alignof", "as", "be", "become", "box", "break", "const", "continue", "crate",
    "do", "else", "enum", "extern", "false", "final", "fn", "for", "if", "impl", "in", "let",
    "loop", "macro", "match", "mod", "move", "mut", "offsetof", "once", "override", "priv", "proc",
    "pub", "pure", "ref", "return", "self", "sizeof", "static", "struct", "super", "trait", "true",
    "type", "typeof", "unsafe", "unsized", "use", "virtual", "where", "while", "yield",
];

fn module_name(camel_case: &str) -> String {
    let mut name = camel_to_snake_case(camel_case);
    if RUST_KEYWORDS.contains(&&*name) {
        name.push('_');
    }
    name
}

// Annotation IDs, as defined in rust.capnp.
const NAME_ANNOTATION_ID: u64 = 0xc2fe4c6d100166d0;
const PARENT_MODULE_ANNOTATION_ID: u64 = 0xabee386cd1450364;
const OPTION_ANNOTATION_ID: u64 = 0xabfef22c4ee1964e;

// StreamResult type ID, as defined in stream.capnp.
const STREAM_RESULT_ID: u64 = 0x995f9a3377c0b16e;

fn name_annotation_value(annotation: schema_capnp::annotation::Reader<'_>) -> capnp::Result<&str> {
    if let schema_capnp::value::Text(t) = annotation.get_value()?.which()? {
        let name = t?.to_str()?;
        for c in name.chars() {
            if !(c == '_' || c.is_alphanumeric()) {
                return Err(capnp::Error::failed(
                    "rust.name annotation value must only contain alphanumeric characters and '_'"
                        .to_string(),
                ));
            }
        }
        Ok(name)
    } else {
        Err(capnp::Error::failed(
            "expected rust.name annotation value to be of type Text".to_string(),
        ))
    }
}

fn get_field_name(field: schema_capnp::field::Reader<'_>) -> capnp::Result<&str> {
    for annotation in field.get_annotations()? {
        if annotation.get_id() == NAME_ANNOTATION_ID {
            return name_annotation_value(annotation);
        }
    }
    Ok(field.get_name()?.to_str()?)
}

fn get_enumerant_name(enumerant: schema_capnp::enumerant::Reader<'_>) -> capnp::Result<&str> {
    for annotation in enumerant.get_annotations()? {
        if annotation.get_id() == NAME_ANNOTATION_ID {
            return name_annotation_value(annotation);
        }
    }
    Ok(enumerant.get_name()?.to_str()?)
}

fn get_parent_module(annotation: schema_capnp::annotation::Reader) -> capnp::Result<Vec<String>> {
    if let schema_capnp::value::Text(t) = annotation.get_value()?.which()? {
        let module = t?.to_str()?;
        Ok(module.split("::").map(|x| x.to_string()).collect())
    } else {
        Err(capnp::Error::failed(
            "expected rust.parentModule annotation value to be of type Text".to_string(),
        ))
    }
}
#[derive(Clone, Copy)]
enum NameKind {
    // convert camel case to snake case, and avoid Rust keywords
    Module,

    // don't modify
    Verbatim,
}

fn capnp_name_to_rust_name(capnp_name: &str, name_kind: NameKind) -> String {
    match name_kind {
        NameKind::Module => module_name(capnp_name),
        NameKind::Verbatim => capnp_name.to_string(),
    }
}

fn is_option_field(field: schema_capnp::field::Reader) -> capnp::Result<bool> {
    use capnp::schema_capnp::*;

    let enabled = field
        .get_annotations()?
        .iter()
        .any(|a| a.get_id() == OPTION_ANNOTATION_ID);

    if enabled {
        let supported = match field.which()? {
            field::Which::Group(_) => false,
            field::Which::Slot(field) => {
                let ty = field.get_type()?;
                ty.is_pointer()? && !matches!(ty.which()?, type_::Interface(_))
            }
        };
        if !supported {
            return Err(capnp::Error::failed(
                "$Rust.option annotation only supported on pointer fields (support for optional interfaces isn't implemented yet)".to_string(),
            ));
        }
    }

    Ok(enabled)
}

fn prim_default(value: &schema_capnp::value::Reader) -> ::capnp::Result<Option<String>> {
    use capnp::schema_capnp::value;
    match value.which()? {
        value::Bool(false)
        | value::Int8(0)
        | value::Int16(0)
        | value::Int32(0)
        | value::Int64(0)
        | value::Uint8(0)
        | value::Uint16(0)
        | value::Uint32(0)
        | value::Uint64(0) => Ok(None),

        value::Bool(true) => Ok(Some("true".to_string())),
        value::Int8(i) => Ok(Some(i.to_string())),
        value::Int16(i) => Ok(Some(i.to_string())),
        value::Int32(i) => Ok(Some(i.to_string())),
        value::Int64(i) => Ok(Some(i.to_string())),
        value::Uint8(i) => Ok(Some(i.to_string())),
        value::Uint16(i) => Ok(Some(i.to_string())),
        value::Uint32(i) => Ok(Some(i.to_string())),
        value::Uint64(i) => Ok(Some(i.to_string())),
        value::Float32(f) => match f.classify() {
            ::std::num::FpCategory::Zero => Ok(None),
            _ => Ok(Some(format!("{}u32", f.to_bits()))),
        },
        value::Float64(f) => match f.classify() {
            ::std::num::FpCategory::Zero => Ok(None),
            _ => Ok(Some(format!("{}u64", f.to_bits()))),
        },
        _ => Err(Error::failed(
            "Non-primitive value found where primitive was expected.".to_string(),
        )),
    }
}

// Gets the full list ordered of generic parameters for a node. Outer scopes come first.
fn get_params(ctx: &GeneratorContext, mut node_id: u64) -> ::capnp::Result<Vec<String>> {
    let mut result = Vec::new();

    while node_id != 0 {
        let node = ctx.node_map[&node_id];
        let parameters = node.get_parameters()?;

        for parameter in parameters.into_iter().rev() {
            result.push(parameter.get_name()?.to_str()?.into());
        }

        node_id = node.get_scope_id();
    }

    result.reverse();
    Ok(result)
}

//
// Returns (type, getter body, default_decl)
//
pub fn getter_text(
    ctx: &GeneratorContext,
    field: &schema_capnp::field::Reader,
    is_reader: bool,
    is_fn: bool,
) -> ::capnp::Result<(String, FormattedText, Option<FormattedText>)> {
    use capnp::schema_capnp::*;

    match field.which()? {
        field::Group(group) => {
            let params = get_params(ctx, group.get_type_id())?;
            let params_string = if params.is_empty() {
                "".to_string()
            } else {
                format!(",{}", params.join(","))
            };

            let the_mod = ctx.get_qualified_module(group.get_type_id());

            let mut result_type = if is_reader {
                format!("{the_mod}::Reader<'a{params_string}>")
            } else {
                format!("{the_mod}::Builder<'a{params_string}>")
            };

            if is_fn {
                result_type = format!("-> {result_type}");
            }

            let getter_code = if is_reader {
                line("self.reader.into()")
            } else {
                line("self.builder.into()")
            };

            Ok((result_type, getter_code, None))
        }
        field::Slot(reg_field) => {
            let mut default_decl = None;
            let offset = reg_field.get_offset() as usize;
            let module_string = if is_reader { "Reader" } else { "Builder" };
            let module = if is_reader {
                Leaf::Reader("'a")
            } else {
                Leaf::Builder("'a")
            };
            let member = camel_to_snake_case(module_string);

            fn primitive_case<T: PartialEq + ::std::fmt::Display>(
                typ: &str,
                member: &str,
                offset: usize,
                default: T,
                zero: T,
            ) -> String {
                if default == zero {
                    format!("self.{member}.get_data_field::<{typ}>({offset})")
                } else {
                    format!("self.{member}.get_data_field_mask::<{typ}>({offset}, {default})")
                }
            }

            let raw_type = reg_field.get_type()?;
            let inner_type = raw_type.type_string(ctx, module)?;
            let default_value = reg_field.get_default_value()?;
            let default = default_value.which()?;
            let default_name = format!(
                "DEFAULT_{}",
                snake_to_upper_case(&camel_to_snake_case(get_field_name(*field)?))
            );
            let should_get_option = is_option_field(*field)?;

            let typ = if should_get_option {
                format!("Option<{inner_type}>")
            } else {
                inner_type
            };

            let (is_fallible, mut result_type) = match raw_type.which()? {
                type_::Enum(_) => (
                    true,
                    fmt!(ctx, "::core::result::Result<{typ},{capnp}::NotInSchema>"),
                ),
                type_::AnyPointer(_) if !raw_type.is_parameter()? => (false, typ.clone()),
                type_::Interface(_) => (
                    true,
                    fmt!(
                        ctx,
                        "{capnp}::Result<{}>",
                        raw_type.type_string(ctx, Leaf::Client)?
                    ),
                ),
                _ if raw_type.is_prim()? => (false, typ.clone()),
                _ => (true, fmt!(ctx, "{capnp}::Result<{typ}>")),
            };

            if is_fn {
                result_type = if result_type == "()" {
                    "".to_string()
                } else {
                    format!("-> {result_type}")
                }
            }

            let getter_fragment = match (raw_type.which()?, default) {
                (type_::Void(()), value::Void(())) => {
                    if is_fn {
                        "".to_string()
                    } else {
                        "()".to_string()
                    }
                }
                (type_::Bool(()), value::Bool(b)) => {
                    if b {
                        format!("self.{member}.get_bool_field_mask({offset}, true)")
                    } else {
                        format!("self.{member}.get_bool_field({offset})")
                    }
                }
                (type_::Int8(()), value::Int8(i)) => primitive_case(&typ, &member, offset, i, 0),
                (type_::Int16(()), value::Int16(i)) => primitive_case(&typ, &member, offset, i, 0),
                (type_::Int32(()), value::Int32(i)) => primitive_case(&typ, &member, offset, i, 0),
                (type_::Int64(()), value::Int64(i)) => primitive_case(&typ, &member, offset, i, 0),
                (type_::Uint8(()), value::Uint8(i)) => primitive_case(&typ, &member, offset, i, 0),
                (type_::Uint16(()), value::Uint16(i)) => {
                    primitive_case(&typ, &member, offset, i, 0)
                }
                (type_::Uint32(()), value::Uint32(i)) => {
                    primitive_case(&typ, &member, offset, i, 0)
                }
                (type_::Uint64(()), value::Uint64(i)) => {
                    primitive_case(&typ, &member, offset, i, 0)
                }
                (type_::Float32(()), value::Float32(f)) => {
                    primitive_case(&typ, &member, offset, f.to_bits(), 0)
                }
                (type_::Float64(()), value::Float64(f)) => {
                    primitive_case(&typ, &member, offset, f.to_bits(), 0)
                }
                (type_::Enum(_), value::Enum(d)) => {
                    if d == 0 {
                        format!("::core::convert::TryFrom::try_from(self.{member}.get_data_field::<u16>({offset}))")
                    } else {
                        format!(
                                "::core::convert::TryFrom::try_from(self.{member}.get_data_field_mask::<u16>({offset}, {d}))")
                    }
                }

                (type_::Text(()), value::Text(_))
                | (type_::Data(()), value::Data(_))
                | (type_::List(_), value::List(_))
                | (type_::Struct(_), value::Struct(_)) => {
                    let default = if reg_field.get_had_explicit_default() {
                        default_decl = Some(crate::pointer_constants::word_array_declaration(
                            ctx,
                            &default_name,
                            ::capnp::raw::get_struct_pointer_section(default_value).get(0),
                            crate::pointer_constants::WordArrayDeclarationOptions {
                                pub_crate: true,
                            },
                        )?);
                        format!("::core::option::Option::Some(&_private::{default_name}[..])")
                    } else {
                        "::core::option::Option::None".to_string()
                    };

                    if is_reader {
                        fmt!(ctx,
                            "{capnp}::traits::FromPointerReader::get_from_pointer(&self.{member}.get_pointer_field({offset}), {default})")
                    } else {
                        fmt!(ctx,"{capnp}::traits::FromPointerBuilder::get_from_pointer(self.{member}.get_pointer_field({offset}), {default})")
                    }
                }

                (type_::Interface(_), value::Interface(_)) => {
                    fmt!(ctx,"match self.{member}.get_pointer_field({offset}).get_capability() {{ ::core::result::Result::Ok(c) => ::core::result::Result::Ok({capnp}::capability::FromClientHook::new(c)), ::core::result::Result::Err(e) => ::core::result::Result::Err(e)}}")
                }
                (type_::AnyPointer(_), value::AnyPointer(_)) => {
                    if !raw_type.is_parameter()? && reg_field.get_had_explicit_default() {
                        // AnyPointer getters are infallible. Generate a validated
                        // pointer constant, borrowing it for readers and copying
                        // it into a null field for builders. A non-null field must
                        // always win over the schema default.
                        default_decl = Some(generate_pointer_constant(
                            ctx,
                            &default_name,
                            raw_type,
                            ::capnp::raw::get_struct_pointer_section(default_value).get(0),
                            true,
                        )?);
                        if is_reader {
                            fmt!(ctx, "{{ let pointer = {capnp}::any_pointer::Reader::new(self.{member}.get_pointer_field({offset})); if pointer.is_null() {{ _private::{default_name}.get().expect(\"validated AnyPointer default\") }} else {{ pointer }} }}")
                        } else {
                            fmt!(ctx, "{{ let mut pointer = {capnp}::any_pointer::Builder::new(self.{member}.get_pointer_field({offset})); if pointer.is_null() {{ pointer.set_as::<{capnp}::any_pointer::Owned>(_private::{default_name}.get().expect(\"validated AnyPointer default\")).expect(\"copy validated AnyPointer default\"); }} pointer }}")
                        }
                    } else if !raw_type.is_parameter()? {
                        fmt!(ctx,"{capnp}::any_pointer::{module_string}::new(self.{member}.get_pointer_field({offset}))")
                    } else if is_reader {
                        fmt!(ctx,"{capnp}::traits::FromPointerReader::get_from_pointer(&self.{member}.get_pointer_field({offset}), ::core::option::Option::None)")
                    } else {
                        fmt!(ctx,"{capnp}::traits::FromPointerBuilder::get_from_pointer(self.{member}.get_pointer_field({offset}), ::core::option::Option::None)")
                    }
                }
                _ => return Err(Error::failed("default value was of wrong type".to_string())),
            };

            let getter_code = if should_get_option {
                Branch(vec![
                    Line(format!(
                        "if self.{member}.is_pointer_field_null({offset}) {{"
                    )),
                    indent(Line(
                        if is_fallible {
                            "core::result::Result::Ok(core::option::Option::None)"
                        } else {
                            "::core::option::Option::None"
                        }
                        .to_string(),
                    )),
                    Line("} else {".to_string()),
                    indent(Line(if is_fallible {
                        format!("{getter_fragment}.map(::core::option::Option::Some)")
                    } else {
                        format!("::core::option::Option::Some({getter_fragment})")
                    })),
                    Line("}".to_string()),
                ])
            } else {
                Line(getter_fragment)
            };

            Ok((result_type, getter_code, default_decl))
        }
    }
}

fn zero_fields_of_group(
    ctx: &GeneratorContext,
    node_id: u64,
    clear: &mut bool,
) -> ::capnp::Result<FormattedText> {
    use capnp::schema_capnp::{field, node, type_};
    match ctx.node_map[&node_id].which()? {
        node::Struct(st) => {
            let mut result = Vec::new();
            if st.get_discriminant_count() != 0 {
                result.push(Line(format!(
                    "self.builder.set_data_field::<u16>({}, 0);",
                    st.get_discriminant_offset()
                )));
            }
            let fields = st.get_fields()?;
            for field in fields {
                match field.which()? {
                    field::Group(group) => {
                        result.push(zero_fields_of_group(ctx, group.get_type_id(), clear)?);
                    }
                    field::Slot(slot) => {
                        let typ = slot.get_type()?.which()?;
                        match typ {
                            type_::Void(()) => {}
                            type_::Bool(()) => {
                                let line = Line(format!(
                                    "self.builder.set_bool_field({}, false);",
                                    slot.get_offset()
                                ));
                                // PERF could dedup more efficiently
                                if !result.contains(&line) {
                                    result.push(line)
                                }
                            }
                            type_::Int8(())
                            | type_::Int16(())
                            | type_::Int32(())
                            | type_::Int64(())
                            | type_::Uint8(())
                            | type_::Uint16(())
                            | type_::Uint32(())
                            | type_::Uint64(())
                            | type_::Float32(())
                            | type_::Float64(()) => {
                                let line = Line(format!(
                                    "self.builder.set_data_field::<{0}>({1}, 0{0});",
                                    slot.get_type()?.type_string(ctx, Leaf::Builder("'a"))?,
                                    slot.get_offset()
                                ));
                                // PERF could dedup more efficiently
                                if !result.contains(&line) {
                                    result.push(line)
                                }
                            }
                            type_::Enum(_) => {
                                let line = Line(format!(
                                    "self.builder.set_data_field::<u16>({}, 0u16);",
                                    slot.get_offset()
                                ));
                                // PERF could dedup more efficiently
                                if !result.contains(&line) {
                                    result.push(line)
                                }
                            }
                            type_::Struct(_)
                            | type_::List(_)
                            | type_::Text(())
                            | type_::Data(())
                            | type_::AnyPointer(_)
                            | type_::Interface(_) => {
                                // Is this the right thing to do for interfaces?
                                let line = Line(format!(
                                    "self.builder.reborrow().get_pointer_field({}).clear();",
                                    slot.get_offset()
                                ));
                                *clear = true;
                                // PERF could dedup more efficiently
                                if !result.contains(&line) {
                                    result.push(line)
                                }
                            }
                        }
                    }
                }
            }
            Ok(Branch(result))
        }
        _ => Err(Error::failed(
            "zero_fields_of_group() expected a struct".to_string(),
        )),
    }
}

fn generate_setter(
    ctx: &GeneratorContext,
    discriminant_offset: u32,
    styled_name: &str,
    field: &schema_capnp::field::Reader,
    doc: &FormattedText,
) -> ::capnp::Result<FormattedText> {
    use capnp::schema_capnp::*;

    let mut setter_interior = Vec::new();
    let mut setter_param = "value".to_string();
    let mut initter_interior = Vec::new();
    let mut initter_mut = false;
    let mut initn_interior = Vec::new();
    let mut initter_params = Vec::new();

    let discriminant_value = field.get_discriminant_value();
    if discriminant_value != field::NO_DISCRIMINANT {
        setter_interior.push(Line(format!(
            "self.builder.set_data_field::<u16>({}, {});",
            discriminant_offset as usize, discriminant_value as usize
        )));
        let init_discrim = Line(format!(
            "self.builder.set_data_field::<u16>({}, {});",
            discriminant_offset as usize, discriminant_value as usize
        ));
        initter_interior.push(init_discrim.clone());
        initn_interior.push(init_discrim);
    }

    let mut return_result = false;
    let mut result = Vec::new();

    let (maybe_reader_type, maybe_builder_type): (Option<String>, Option<String>) = match field
        .which()?
    {
        field::Group(group) => {
            let params = get_params(ctx, group.get_type_id())?;
            let params_string = if params.is_empty() {
                "".to_string()
            } else {
                format!(",{}", params.join(","))
            };

            let the_mod = ctx.get_qualified_module(group.get_type_id());

            initter_interior.push(zero_fields_of_group(
                ctx,
                group.get_type_id(),
                &mut initter_mut,
            )?);

            initter_interior.push(line("self.builder.into()"));

            (None, Some(format!("{the_mod}::Builder<'a{params_string}>")))
        }
        field::Slot(reg_field) => {
            let offset = reg_field.get_offset() as usize;
            let typ = reg_field.get_type()?;
            match typ.which().expect("unrecognized type") {
                type_::Void(()) => {
                    setter_param = "_value".to_string();
                    (Some("()".to_string()), None)
                }
                type_::Bool(()) => {
                    match prim_default(&reg_field.get_default_value()?)? {
                        None => {
                            setter_interior.push(Line(format!(
                                "self.builder.set_bool_field({offset}, value);"
                            )));
                        }
                        Some(s) => {
                            setter_interior.push(Line(format!(
                                "self.builder.set_bool_field_mask({offset}, value, {s});"
                            )));
                        }
                    }
                    (Some("bool".to_string()), None)
                }
                _ if typ.is_prim()? => {
                    let tstr = typ.type_string(ctx, Leaf::Reader("'a"))?;
                    match prim_default(&reg_field.get_default_value()?)? {
                        None => {
                            setter_interior.push(Line(format!(
                                "self.builder.set_data_field::<{tstr}>({offset}, value);"
                            )));
                        }
                        Some(s) => {
                            setter_interior.push(Line(format!(
                                "self.builder.set_data_field_mask::<{tstr}>({offset}, value, {s});"
                            )));
                        }
                    };
                    (Some(tstr), None)
                }
                type_::Text(()) => {
                    // The text::Reader impl of SetterInput never fails, so we can unwrap().
                    setter_interior.push(Line(fmt!(ctx,
                        "{capnp}::traits::SetterInput::set_pointer_builder(self.builder.reborrow().get_pointer_field({offset}), value, false).unwrap()"
                    )));
                    initter_interior.push(Line(format!(
                        "self.builder.get_pointer_field({offset}).init_text(size)"
                    )));
                    initter_params.push("size: u32");
                    (
                        Some(fmt!(
                            ctx,
                            "impl {capnp}::traits::SetterInput<{capnp}::text::Owned>"
                        )),
                        Some(fmt!(ctx, "{capnp}::text::Builder<'a>")),
                    )
                }
                type_::Data(()) => {
                    setter_interior.push(Line(format!(
                        "self.builder.reborrow().get_pointer_field({offset}).set_data(value);"
                    )));
                    initter_interior.push(Line(format!(
                        "self.builder.get_pointer_field({offset}).init_data(size)"
                    )));
                    initter_params.push("size: u32");
                    (
                        Some(fmt!(ctx, "{capnp}::data::Reader<'_>")),
                        Some(fmt!(ctx, "{capnp}::data::Builder<'a>")),
                    )
                }
                type_::List(ls) => {
                    let et = ls.get_element_type()?;
                    return_result = true;
                    setter_interior.push(
                        Line(fmt!(ctx,"{capnp}::traits::SetterInput::set_pointer_builder(self.builder.reborrow().get_pointer_field({offset}), value, false)")));

                    initter_params.push("size: u32");
                    initter_interior.push(
                        Line(fmt!(ctx,"{capnp}::traits::FromPointerBuilder::init_pointer(self.builder.get_pointer_field({offset}), size)")));

                    let mr = match et.which()? {
                        type_::Void(())
                        | type_::Bool(())
                        | type_::Int8(())
                        | type_::Int16(())
                        | type_::Int32(())
                        | type_::Int64(())
                        | type_::Uint8(())
                        | type_::Uint16(())
                        | type_::Uint32(())
                        | type_::Uint64(())
                        | type_::Float32(())
                        | type_::Float64(())
                        | type_::Enum(_)
                        | type_::Text(()) => {
                            // There are multiple SetterInput impls.
                            fmt!(
                                ctx,
                                "impl {capnp}::traits::SetterInput<{}>",
                                reg_field.get_type()?.type_string(ctx, Leaf::Owned)?
                            )
                        }
                        _ => reg_field.get_type()?.type_string(ctx, Leaf::Reader("'_"))?,
                    };

                    (
                        Some(mr),
                        Some(
                            reg_field
                                .get_type()?
                                .type_string(ctx, Leaf::Builder("'a"))?,
                        ),
                    )
                }
                type_::Enum(e) => {
                    let id = e.get_type_id();
                    let the_mod = ctx.get_qualified_module(id);
                    if !reg_field.get_had_explicit_default() {
                        setter_interior.push(Line(format!(
                            "self.builder.set_data_field::<u16>({offset}, value as u16);"
                        )));
                    } else {
                        match reg_field.get_default_value()?.which()? {
                            schema_capnp::value::Enum(d) => {
                                setter_interior.push(Line(format!(
                                    "self.builder.set_data_field_mask::<u16>({offset}, value as u16, {d});"
                                )));
                            }
                            _ => return Err(Error::failed("enum default not an Enum".to_string())),
                        }
                    };
                    (Some(the_mod), None)
                }
                type_::Struct(_) => {
                    return_result = true;
                    initter_interior.push(
                      Line(fmt!(ctx,"{capnp}::traits::FromPointerBuilder::init_pointer(self.builder.get_pointer_field({offset}), 0)")));
                    setter_interior.push(
                        Line(fmt!(ctx,"{capnp}::traits::SetterInput::set_pointer_builder(self.builder.reborrow().get_pointer_field({offset}), value, false)")));

                    (
                        Some(typ.type_string(ctx, Leaf::Reader("'_"))?),
                        Some(typ.type_string(ctx, Leaf::Builder("'a"))?),
                    )
                }
                type_::Interface(_) => {
                    setter_interior.push(Line(format!(
                        "self.builder.reborrow().get_pointer_field({offset}).set_capability(value.client.hook);"
                    )));
                    (Some(typ.type_string(ctx, Leaf::Client)?), None)
                }
                type_::AnyPointer(_) => {
                    if typ.is_parameter()? {
                        initter_interior.push(Line(fmt!(ctx,"{capnp}::any_pointer::Builder::new(self.builder.get_pointer_field({offset})).init_as()")));
                        setter_interior.push(Line(fmt!(ctx,"{capnp}::traits::SetterInput::set_pointer_builder(self.builder.reborrow().get_pointer_field({offset}), value, false)")));
                        return_result = true;

                        let builder_type = typ.type_string(ctx, Leaf::Builder("'a"))?;

                        result.push(doc.clone());
                        result.push(line("#[inline]"));
                        result.push(Line(format!(
                            "pub fn initn_{styled_name}(self, length: u32) -> {builder_type} {{"
                        )));
                        result.push(indent(initn_interior));
                        result.push(indent(
                            Line(fmt!(ctx,"{capnp}::any_pointer::Builder::new(self.builder.get_pointer_field({offset})).initn_as(length)")))
                        );
                        result.push(line("}"));

                        (
                            Some(fmt!(
                                ctx,
                                "impl {capnp}::traits::SetterInput<{}>",
                                typ.type_string(ctx, Leaf::Owned)?
                            )),
                            Some(builder_type),
                        )
                    } else {
                        initter_interior.push(Line(fmt!(ctx,"let mut result = {capnp}::any_pointer::Builder::new(self.builder.get_pointer_field({offset}));")));
                        initter_interior.push(line("result.clear();"));
                        initter_interior.push(line("result"));
                        (None, Some(fmt!(ctx, "{capnp}::any_pointer::Builder<'a>")))
                    }
                }
                _ => return Err(Error::failed("unrecognized type".to_string())),
            }
        }
    };
    if let Some(reader_type) = maybe_reader_type {
        let return_type = if return_result {
            fmt!(ctx, "-> {capnp}::Result<()>")
        } else {
            "".into()
        };
        result.push(doc.clone());
        result.push(line("#[inline]"));
        result.push(Line(format!(
            "pub fn set_{styled_name}(&mut self, {setter_param}: {reader_type}) {return_type} {{"
        )));
        result.push(indent(setter_interior));
        result.push(line("}"));
    }
    if let Some(builder_type) = maybe_builder_type {
        result.push(doc.clone());
        result.push(line("#[inline]"));
        let args = initter_params.join(", ");
        let mutable = if initter_mut { "mut " } else { "" };
        result.push(Line(format!(
            "pub fn init_{styled_name}({mutable}self, {args}) -> {builder_type} {{"
        )));
        result.push(indent(initter_interior));
        result.push(line("}"));
    }
    Ok(Branch(result))
}

fn used_params_of_group(
    ctx: &GeneratorContext,
    group_id: u64,
    used_params: &mut HashSet<String>,
) -> capnp::Result<()> {
    let node = ctx.node_map[&group_id];
    match node.which()? {
        schema_capnp::node::Struct(st) if st.get_is_group() => {
            // Generated group Reader/Builder types carry every parameter of
            // their containing scopes, including parameters unused by fields.
            // A union alias naming that group must bind the same parameters.
            used_params.extend(node.parameters_texts(ctx).expanded_list);
            Ok(())
        }
        _ => Err(Error::failed("not a group".to_string())),
    }
}

fn used_params_of_type(
    ctx: &GeneratorContext,
    ty: schema_capnp::type_::Reader,
    used_params: &mut HashSet<String>,
) -> capnp::Result<()> {
    use capnp::schema_capnp::type_;
    match ty.which()? {
        type_::List(ls) => {
            let et = ls.get_element_type()?;
            used_params_of_type(ctx, et, used_params)?;
        }
        type_::Enum(e) => {
            let node_id = e.get_type_id();
            let brand = e.get_brand()?;
            used_params_of_brand(ctx, node_id, brand, used_params)?;
        }
        type_::Struct(s) => {
            let node_id = s.get_type_id();
            let brand = s.get_brand()?;
            used_params_of_brand(ctx, node_id, brand, used_params)?;
        }
        type_::Interface(i) => {
            let node_id = i.get_type_id();
            let brand = i.get_brand()?;
            used_params_of_brand(ctx, node_id, brand, used_params)?;
        }

        type_::AnyPointer(ap) => {
            if let type_::any_pointer::Parameter(def) = ap.which()? {
                let the_struct = &ctx.node_map[&def.get_scope_id()];
                let parameters = the_struct.get_parameters()?;
                let parameter = parameters.get(u32::from(def.get_parameter_index()));
                let parameter_name = parameter.get_name()?.to_str()?;
                used_params.insert(parameter_name.to_string());
            }
        }
        _ => (),
    }
    Ok(())
}

fn used_params_of_brand(
    ctx: &GeneratorContext,
    node_id: u64,
    brand: schema_capnp::brand::Reader,
    used_params: &mut HashSet<String>,
) -> capnp::Result<()> {
    use schema_capnp::brand;
    let scopes = brand.get_scopes()?;
    let mut brand_scopes = HashMap::new();
    for scope in scopes {
        brand_scopes.insert(scope.get_scope_id(), scope);
    }
    let brand_scopes = brand_scopes; // freeze
    let mut current_node_id = node_id;
    while let Some(current_node) = ctx.node_map.get(&current_node_id) {
        let params = current_node.get_parameters()?;
        match brand_scopes.get(&current_node_id) {
            None => (),
            Some(scope) => match scope.which()? {
                brand::scope::Inherit(()) => {
                    for param in params {
                        used_params.insert(param.get_name()?.to_string()?);
                    }
                }
                brand::scope::Bind(bindings_list_opt) => {
                    let bindings_list = bindings_list_opt?;
                    assert_eq!(bindings_list.len(), params.len());
                    for binding in bindings_list {
                        match binding.which()? {
                            brand::binding::Unbound(()) => (),
                            brand::binding::Type(t) => {
                                used_params_of_type(ctx, t?, used_params)?;
                            }
                        }
                    }
                }
            },
        }
        current_node_id = current_node.get_scope_id();
    }
    Ok(())
}

// return (the 'Which' enum, the 'which()' accessor, typedef, default_decls)
fn generate_union(
    ctx: &GeneratorContext,
    node_id: u64,
    discriminant_offset: u32,
    fields: &[(usize, schema_capnp::field::Reader)],
    is_reader: bool,
    params: &TypeParameterTexts,
) -> ::capnp::Result<(
    FormattedText,
    FormattedText,
    FormattedText,
    Vec<FormattedText>,
)> {
    use capnp::schema_capnp::*;

    fn new_ty_param(ty_params: &mut Vec<String>) -> String {
        let result = format!("A{}", ty_params.len());
        ty_params.push(result.clone());
        result
    }

    let mut getter_interior = Vec::new();
    let mut interior = Vec::new();
    let mut enum_interior = Vec::new();
    let mut default_decls = Vec::new();

    let mut ty_params = Vec::new();
    let mut ty_args = Vec::new();

    let mut used_params: HashSet<String> = HashSet::new();

    let doffset = discriminant_offset as usize;

    for (index, field) in fields {
        let dvalue = field.get_discriminant_value() as usize;

        let field_name = get_field_name(*field)?;
        let enumerant_name = capitalize_first_letter(field_name);

        let (ty, get, maybe_default_decl) = getter_text(ctx, field, is_reader, false)?;
        if let Some(default_decl) = maybe_default_decl {
            default_decls.push(default_decl)
        }

        getter_interior.push(Branch(vec![
            Line(format!("{dvalue} => {{")),
            indent(Line(format!(
                "::core::result::Result::Ok({}(",
                enumerant_name.clone()
            ))),
            indent(indent(get)),
            indent(line("))")),
            line("}"),
        ]));

        let ty1 = match field.which() {
            Ok(field::Group(group)) => {
                used_params_of_group(ctx, group.get_type_id(), &mut used_params)?;
                ty_args.push(ty);
                new_ty_param(&mut ty_params)
            }
            Ok(field::Slot(reg_field)) => {
                let fty = reg_field.get_type()?;
                used_params_of_type(ctx, fty, &mut used_params)?;
                match fty.which() {
                    Ok(
                        type_::Text(())
                        | type_::Data(())
                        | type_::List(_)
                        | type_::Struct(_)
                        | type_::AnyPointer(_),
                    ) => {
                        ty_args.push(ty);
                        new_ty_param(&mut ty_params)
                    }
                    Ok(type_::Interface(_)) => ty,
                    _ => ty,
                }
            }
            _ => ty,
        };

        enum_interior.push(ctx.documentation.formatted(node_id, Some(*index)));
        enum_interior.push(Line(format!("{enumerant_name}({ty1}),")));
    }

    let enum_name = format!(
        "Which{}",
        if !ty_params.is_empty() {
            format!("<{}>", ty_params.join(","))
        } else {
            "".to_string()
        }
    );

    getter_interior.push(Line(fmt!(
        ctx,
        "x => ::core::result::Result::Err({capnp}::NotInSchema(x))"
    )));

    interior.push(Branch(vec![
        Line(format!("pub enum {enum_name} {{")),
        indent(enum_interior),
        line("}"),
    ]));

    let result = Branch(interior);

    let field_name = if is_reader { "reader" } else { "builder" };

    let concrete_type = format!(
        "Which{}{}",
        if is_reader { "Reader" } else { "Builder" },
        if !ty_params.is_empty() {
            format!(
                "<'a,{}>",
                params
                    .expanded_list
                    .iter()
                    .filter(|s: &&String| used_params.contains(*s))
                    .cloned()
                    .collect::<Vec<String>>()
                    .join(",")
            )
        } else {
            "".to_string()
        }
    );

    let typedef = Line(format!(
        "pub type {concrete_type} = Which{};",
        if !ty_args.is_empty() {
            format!("<{}>", ty_args.join(","))
        } else {
            "".to_string()
        }
    ));

    let getter_result = Branch(vec![
        line("#[inline]"),
        Line(fmt!(ctx,
            "pub fn which(self) -> ::core::result::Result<{concrete_type}, {capnp}::NotInSchema> {{"
        )),
        indent(vec![
            Line(format!(
                "match self.{field_name}.get_data_field::<u16>({doffset}) {{"
            )),
            indent(getter_interior),
            line("}"),
        ]),
        line("}"),
    ]);

    // TODO set_which() for builders?

    Ok((result, getter_result, typedef, default_decls))
}

fn generate_haser(
    discriminant_offset: u32,
    styled_name: &str,
    field: &schema_capnp::field::Reader,
    is_reader: bool,
) -> ::capnp::Result<FormattedText> {
    use capnp::schema_capnp::*;

    let mut result = Vec::new();
    let mut interior = Vec::new();
    let member = if is_reader { "reader" } else { "builder" };

    let discriminant_value = field.get_discriminant_value();
    if discriminant_value != field::NO_DISCRIMINANT {
        interior.push(Line(format!(
            "if self.{}.get_data_field::<u16>({}) != {} {{ return false; }}",
            member, discriminant_offset as usize, discriminant_value as usize
        )));
    }
    match field.which() {
        Err(_) | Ok(field::Group(_)) => {}
        Ok(field::Slot(reg_field)) => match reg_field.get_type()?.which()? {
            type_::Text(())
            | type_::Data(())
            | type_::List(_)
            | type_::Struct(_)
            | type_::Interface(_)
            | type_::AnyPointer(_) => {
                if is_reader {
                    interior.push(Line(format!(
                        "!self.{member}.get_pointer_field({}).is_null()",
                        reg_field.get_offset()
                    )));
                } else {
                    interior.push(Line(format!(
                        "!self.{member}.is_pointer_field_null({})",
                        reg_field.get_offset()
                    )));
                }
                result.push(line("#[inline]"));
                result.push(Line(format!("pub fn has_{styled_name}(&self) -> bool {{")));
                result.push(indent(interior));
                result.push(line("}"));
            }
            _ => {}
        },
    }

    Ok(Branch(result))
}

fn generate_pipeline_getter(
    ctx: &GeneratorContext,
    field: schema_capnp::field::Reader,
) -> ::capnp::Result<FormattedText> {
    use capnp::schema_capnp::{field, type_};

    let name = get_field_name(field)?;

    match field.which()? {
        field::Group(group) => {
            let params = get_params(ctx, group.get_type_id())?;
            let params_string = if params.is_empty() {
                "".to_string()
            } else {
                format!("<{}>", params.join(","))
            };

            let the_mod = ctx.get_qualified_module(group.get_type_id());
            Ok(Branch(vec![
                Line(format!(
                    "pub fn get_{}(&self) -> {}::Pipeline{} {{",
                    camel_to_snake_case(name),
                    the_mod,
                    params_string
                )),
                indent(Line(fmt!(
                    ctx,
                    "{capnp}::capability::FromTypelessPipeline::new(self._typeless.noop())"
                ))),
                line("}"),
            ]))
        }
        field::Slot(reg_field) => {
            let typ = reg_field.get_type()?;
            match typ.which()? {
                type_::Struct(_) | type_::AnyPointer(_) => {
                    Ok(Branch(vec![
                        Line(format!("pub fn get_{}(&self) -> {} {{", camel_to_snake_case(name), typ.type_string(ctx, Leaf::Pipeline)?)),
                        indent(Line(fmt!(ctx,"{capnp}::capability::FromTypelessPipeline::new(self._typeless.get_pointer_field({}))", reg_field.get_offset()))),
                        line("}")
                    ]))
                }
                type_::Interface(_) => {
                    Ok(Branch(vec![
                        Line(format!("pub fn get_{}(&self) -> {} {{", camel_to_snake_case(name), typ.type_string(ctx, Leaf::Client)?)),
                        indent(Line(fmt!(ctx,"{capnp}::capability::FromClientHook::new(self._typeless.get_pointer_field({}).as_cap())", reg_field.get_offset()))),
                        line("}")
                    ]))
                }
                _ => {
                    Ok(Branch(Vec::new()))
                }
            }
        }
    }
}

fn generate_get_field_types(
    ctx: &GeneratorContext,
    node_reader: schema_capnp::node::Reader,
) -> ::capnp::Result<FormattedText> {
    use capnp::schema_capnp::field;
    let st = match node_reader.which()? {
        schema_capnp::node::Struct(st) => st,
        _ => return Err(Error::failed("not a struct".into())),
    };
    let mut branches = vec![];
    for (index, field) in st.get_fields()?.iter().enumerate() {
        match field.which()? {
            field::Slot(slot) => {
                let raw_type = slot.get_type()?;
                let typ = raw_type.type_string(ctx, Leaf::Owned)?;
                branches.push(Line(fmt!(
                    ctx,
                    "{} => <{} as {capnp}::introspect::Introspect>::introspect(),",
                    index,
                    typ
                )));
            }
            field::Group(group) => {
                let params = get_params(ctx, group.get_type_id())?;
                let params_string = if params.is_empty() {
                    "".to_string()
                } else {
                    format!("<{}>", params.join(","))
                };

                let the_mod = ctx.get_qualified_module(group.get_type_id());

                let typ = format!("{the_mod}::Owned{params_string}");
                branches.push(Line(fmt!(
                    ctx,
                    "{} => <{} as {capnp}::introspect::Introspect>::introspect(),",
                    index,
                    typ
                )));
            }
        }
    }
    let body = if branches.is_empty() {
        Line(fmt!(
            ctx,
            "{capnp}::introspect::panic_invalid_field_index(index)"
        ))
    } else {
        branches.push(Line(fmt!(
            ctx,
            "_ => {capnp}::introspect::panic_invalid_field_index(index),"
        )));
        Branch(vec![
            Line("match index {".into()),
            indent(branches),
            Line("}".into()),
        ])
    };
    if !node_reader.get_is_generic() {
        Ok(Branch(vec![
            Line(fmt!(
                ctx,
                "pub(crate) fn get_field_types(index: u16) -> {capnp}::introspect::Type {{"
            )),
            indent(body),
            Line("}".into()),
        ]))
    } else {
        let params = node_reader.parameters_texts(ctx);
        Ok(Branch(vec![
            Line(fmt!(
                ctx,
                "pub(crate) fn get_field_types<{0}>(index: u16) -> {capnp}::introspect::Type {1} {{",
                params.params,
                params.where_clause
            )),
            indent(body),
            Line("}".into()),
        ]))
    }
}

fn annotation_branch(
    ctx: &GeneratorContext,
    annotation: schema_capnp::annotation::Reader,
    child_index: Option<u16>,
    index: u32,
) -> ::capnp::Result<FormattedText> {
    use capnp::schema_capnp::node;
    let id = annotation.get_id();
    let annotation_decl = ctx.node_map[&id];
    let node::Annotation(a) = annotation_decl.which()? else {
        return Err(Error::failed("not an annotation node".into()));
    };
    if annotation_decl.get_is_generic() {
        let brand = annotation.get_brand()?;
        let the_mod = ctx.get_qualified_module(id);
        let func = do_branding(ctx, id, brand, Leaf::GetType, &the_mod)?;
        Ok(Line(format!("({child_index:?}, {index}) => {func}(),")))
    } else {
        // Avoid referring to the annotation in the generated code, so that users can import
        // annotation schemas like `c++.capnp` or `rust.capnp` without needing to generate code
        // for them, as long as the annotations are not generic.
        let ty = a.get_type()?;
        Ok(Line(fmt!(
            ctx,
            "({child_index:?}, {index}) => <{} as {capnp}::introspect::Introspect>::introspect(),",
            ty.type_string(ctx, Leaf::Owned)?
        )))
    }
}

fn generate_get_annotation_types(
    ctx: &GeneratorContext,
    node_reader: schema_capnp::node::Reader,
) -> ::capnp::Result<FormattedText> {
    use capnp::schema_capnp::node;

    let mut branches = vec![];

    for (idx, annotation) in node_reader.get_annotations()?.iter().enumerate() {
        branches.push(annotation_branch(ctx, annotation, None, idx as u32)?);
    }

    match node_reader.which()? {
        node::Struct(s) => {
            for (fidx, field) in s.get_fields()?.iter().enumerate() {
                for (idx, annotation) in field.get_annotations()?.iter().enumerate() {
                    branches.push(annotation_branch(
                        ctx,
                        annotation,
                        Some(fidx as u16),
                        idx as u32,
                    )?);
                }
            }
        }
        node::Enum(e) => {
            for (fidx, enumerant) in e.get_enumerants()?.iter().enumerate() {
                for (idx, annotation) in enumerant.get_annotations()?.iter().enumerate() {
                    branches.push(annotation_branch(
                        ctx,
                        annotation,
                        Some(fidx as u16),
                        idx as u32,
                    )?);
                }
            }
        }
        _ => (),
    }

    let body = if branches.is_empty() {
        Line(fmt!(
            ctx,
            "{capnp}::introspect::panic_invalid_annotation_indices(child_index, index)"
        ))
    } else {
        branches.push(Line(fmt!(
            ctx,
            "_ => {capnp}::introspect::panic_invalid_annotation_indices(child_index, index),"
        )));
        indent(vec![
            Line("match (child_index, index) {".into()),
            indent(branches),
            Line("}".into()),
        ])
    };

    if !node_reader.get_is_generic() {
        Ok(Branch(vec![
            Line(fmt!(ctx,"pub(crate) fn get_annotation_types(child_index: Option<u16>, index: u32) -> {capnp}::introspect::Type {{")),
            indent(body),
            Line("}".into()),
        ]))
    } else {
        let params = node_reader.parameters_texts(ctx);
        Ok(Branch(vec![
            Line(fmt!(ctx,
                "pub(crate) fn get_annotation_types<{0}>(child_index: Option<u16>, index: u32) -> {capnp}::introspect::Type {1} {{",
                params.params, params.where_clause
            )),
            indent(body),
            Line("}".into()),
        ]))
    }
}

/// Determine the conservative capability reachability used for RPC call hints.
/// Like C++ SchemaLoader::computeOptimizationHints(), generic parameters and
/// AnyPointer may contain capabilities, regardless of a particular binding.
#[doc(hidden)]
pub fn schema_may_contain_capabilities(
    ctx: &GeneratorContext<'_>,
    root: u64,
) -> capnp::Result<bool> {
    use capnp::schema_capnp::{field, node, type_};
    let mut pending = vec![root];
    let mut visited = collections::HashSet::new();
    while let Some(id) = pending.pop() {
        if !visited.insert(id) {
            continue;
        }
        let Some(schema) = ctx.node_map.get(&id) else {
            return Ok(true);
        };
        let node::Struct(schema) = schema.which()? else {
            return Ok(true);
        };
        for field in schema.get_fields()? {
            match field.which()? {
                field::Group(group) => pending.push(group.get_type_id()),
                field::Slot(slot) => {
                    let mut ty = slot.get_type()?;
                    while let type_::List(list) = ty.which()? {
                        ty = list.get_element_type()?;
                    }
                    match ty.which()? {
                        type_::Interface(_) | type_::AnyPointer(_) => return Ok(true),
                        type_::Struct(structure) => pending.push(structure.get_type_id()),
                        _ => (),
                    }
                }
            }
        }
    }
    Ok(false)
}

fn generate_members_by_discriminant(
    node_reader: schema_capnp::node::Reader,
) -> ::capnp::Result<FormattedText> {
    use capnp::schema_capnp::field;
    let st = match node_reader.which()? {
        schema_capnp::node::Struct(st) => st,
        _ => return Err(Error::failed("not a struct".into())),
    };

    let mut union_member_indexes = vec![];
    let mut nonunion_member_indexes = vec![];
    for (index, field) in st.get_fields()?.iter().enumerate() {
        let disc = field.get_discriminant_value();
        if disc == field::NO_DISCRIMINANT {
            nonunion_member_indexes.push(index);
        } else {
            union_member_indexes.push((disc, index));
        }
    }
    union_member_indexes.sort();

    let mut nonunion_string: String = "pub(crate) static NONUNION_MEMBERS : &[u16] = &[".into();
    for idx in 0..nonunion_member_indexes.len() {
        nonunion_string += &format!("{}", nonunion_member_indexes[idx]);
        if idx + 1 < nonunion_member_indexes.len() {
            nonunion_string += ",";
        }
    }
    nonunion_string += "];";

    let mut members_by_disc: String =
        "pub(crate) static MEMBERS_BY_DISCRIMINANT : &[u16] = &[".into();
    for idx in 0..union_member_indexes.len() {
        let (disc, index) = union_member_indexes[idx];
        assert_eq!(idx, disc as usize);
        members_by_disc += &format!("{index}");
        if idx + 1 < union_member_indexes.len() {
            members_by_disc += ",";
        }
    }
    members_by_disc += "];";
    Ok(Branch(vec![Line(nonunion_string), Line(members_by_disc)]))
}

fn generate_members_by_name(
    node_reader: schema_capnp::node::Reader,
) -> ::capnp::Result<FormattedText> {
    let st = match node_reader.which()? {
        schema_capnp::node::Struct(st) => st,
        _ => return Err(Error::failed("not a struct".into())),
    };

    let mut members_by_name = Vec::new();
    for (index, field) in st.get_fields()?.iter().enumerate() {
        // Reflection compares the original names in the encoded schema. Rust
        // rename annotations affect accessors, not this binary-search index.
        members_by_name.push((field.get_name()?.to_str()?, index));
    }
    members_by_name.sort_by_key(|k| k.0);

    let mut members_by_name_string: String =
        "pub(crate) static MEMBERS_BY_NAME : &[u16] = &[".into();
    for (i, (_, index)) in members_by_name.iter().enumerate() {
        members_by_name_string += &format!("{}", *index);
        if i + 1 < members_by_name.len() {
            members_by_name_string += ",";
        }
    }
    members_by_name_string += "];";

    Ok(Branch(vec![Line(members_by_name_string)]))
}

// We need this to work around the fact that Rust does not allow typedefs
// with unused type parameters.
fn get_ty_params_of_brand(
    ctx: &GeneratorContext,
    brand: schema_capnp::brand::Reader,
    declaring_scope: u64,
) -> ::capnp::Result<String> {
    let mut acc = HashSet::new();
    get_ty_params_of_brand_helper(ctx, &mut acc, brand)?;
    let mut result = String::new();
    // Alias declarations and their uses must agree on parameter order. HashSet
    // iteration is independently randomized on each invocation. Follow lexical
    // declaration order, including parameters inherited from enclosing scopes.
    let mut scopes = Vec::new();
    let mut scope_id = declaring_scope;
    while scope_id != 0 {
        if scopes.contains(&scope_id) {
            return Err(::capnp::Error::failed("cyclic generic scope".into()));
        }
        scopes.push(scope_id);
        scope_id = ctx.node_map[&scope_id].get_scope_id();
    }
    for scope_id in scopes.into_iter().rev() {
        let node = ctx.node_map[&scope_id];
        for (parameter_index, p) in node.get_parameters()?.iter().enumerate() {
            if acc.remove(&(scope_id, parameter_index as u16)) {
                result.push_str(p.get_name()?.to_str()?);
                result.push(',');
            }
        }
    }
    if !acc.is_empty() {
        return Err(::capnp::Error::failed(
            "generic parameter outside lexical scope".into(),
        ));
    }

    Ok(result)
}

fn get_ty_params_of_type_helper(
    ctx: &GeneratorContext,
    accumulator: &mut HashSet<(u64, u16)>,
    typ: schema_capnp::type_::Reader,
) -> ::capnp::Result<()> {
    use capnp::schema_capnp::type_;
    match typ.which()? {
        type_::Void(())
        | type_::Bool(())
        | type_::Int8(())
        | type_::Int16(())
        | type_::Int32(())
        | type_::Int64(())
        | type_::Uint8(())
        | type_::Uint16(())
        | type_::Uint32(())
        | type_::Uint64(())
        | type_::Float32(())
        | type_::Float64(())
        | type_::Text(_)
        | type_::Data(_) => {}
        type_::AnyPointer(p) => {
            match p.which()? {
                type_::any_pointer::Unconstrained(_) => (),
                type_::any_pointer::Parameter(p) => {
                    accumulator.insert((p.get_scope_id(), p.get_parameter_index()));
                }
                type_::any_pointer::ImplicitMethodParameter(_) => {
                    // XXX
                }
            }
        }
        type_::List(list) => {
            get_ty_params_of_type_helper(ctx, accumulator, list.get_element_type()?)?
        }
        type_::Enum(e) => {
            get_ty_params_of_brand_helper(ctx, accumulator, e.get_brand()?)?;
        }
        type_::Struct(s) => {
            get_ty_params_of_brand_helper(ctx, accumulator, s.get_brand()?)?;
        }
        type_::Interface(interf) => {
            get_ty_params_of_brand_helper(ctx, accumulator, interf.get_brand()?)?;
        }
    }
    Ok(())
}

fn get_ty_params_of_brand_helper(
    ctx: &GeneratorContext,
    accumulator: &mut HashSet<(u64, u16)>,
    brand: schema_capnp::brand::Reader,
) -> ::capnp::Result<()> {
    for scope in brand.get_scopes()? {
        let scope_id = scope.get_scope_id();
        match scope.which()? {
            schema_capnp::brand::scope::Bind(bind) => {
                for binding in bind? {
                    match binding.which()? {
                        schema_capnp::brand::binding::Unbound(()) => {}
                        schema_capnp::brand::binding::Type(t) => {
                            get_ty_params_of_type_helper(ctx, accumulator, t?)?
                        }
                    }
                }
            }
            schema_capnp::brand::scope::Inherit(()) => {
                let parameters = ctx.node_map[&scope_id].get_parameters()?;
                for idx in 0..parameters.len() {
                    accumulator.insert((scope_id, idx as u16));
                }
            }
        }
    }
    Ok(())
}

fn generate_brand(ctx: &GeneratorContext, node: schema_capnp::node::Reader<'_>) -> FormattedText {
    let params = node.parameters_texts(ctx);
    let mut arms: Vec<_> = params
        .expanded_list
        .iter()
        .enumerate()
        .map(|(index, param)| {
            Line(fmt!(
                ctx,
                "{index} => <{param} as {capnp}::introspect::Introspect>::introspect(),"
            ))
        })
        .collect();
    arms.push(Line(fmt!(
        ctx,
        "_ => {capnp}::introspect::panic_invalid_field_index(index),"
    )));
    Branch(vec![
        line("#[allow(clippy::match_single_binding, clippy::extra_unused_type_parameters)]"),
        Line(fmt!(
            ctx,
            "pub(crate) fn get_brand_parameter<{}>(index: u16) -> {capnp}::introspect::Type {} {{",
            params.params,
            params.where_clause
        )),
        indent(line("match index {")),
        indent(indent(arms)),
        indent(line("}")),
        line("}"),
    ])
}
