use crate::brand::{Brand, Scope};
use crate::source::Graph;
use crate::syntax::{
    Annotation, AnnotationTarget, Expr, ExprKind, FieldKind, Node, NodeKind, ParamList, Span, Type,
    TypeKind,
};
use crate::value::{TypedValue, Value};
use crate::Diagnostic;
use capnp::schema_capnp::type_;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Resolved {
    Void,
    Bool,
    Int8,
    Int16,
    Int32,
    Int64,
    UInt8,
    UInt16,
    UInt32,
    UInt64,
    Float32,
    Float64,
    Text,
    Data,
    AnyPointer,
    Capability,
    AnyStruct,
    AnyList,
    Parameter(usize, u16),
    ImplicitParameter(u16),
    Interface(usize, Brand),
    List(Box<Resolved>),
    Enum(usize, Brand),
    Struct(usize, Brand),
    Namespace(usize),
    Constant(usize, Brand),
    Annotation(usize, Brand),
    ListConstructor,
}

impl Resolved {
    pub fn data_bits(&self) -> Option<usize> {
        Some(match self {
            Self::Void => 0,
            Self::Bool => 1,
            Self::Int8 | Self::UInt8 => 8,
            Self::Int16 | Self::UInt16 | Self::Enum(..) => 16,
            Self::Int32 | Self::UInt32 | Self::Float32 => 32,
            Self::Int64 | Self::UInt64 | Self::Float64 => 64,
            _ => return None,
        })
    }

    fn builtin(name: &str) -> Option<Self> {
        Some(match name {
            "Void" => Self::Void,
            "Bool" => Self::Bool,
            "Int8" => Self::Int8,
            "Int16" => Self::Int16,
            "Int32" => Self::Int32,
            "Int64" => Self::Int64,
            "UInt8" => Self::UInt8,
            "UInt16" => Self::UInt16,
            "UInt32" => Self::UInt32,
            "UInt64" => Self::UInt64,
            "Float32" => Self::Float32,
            "Float64" => Self::Float64,
            "Text" => Self::Text,
            "Data" => Self::Data,
            "AnyPointer" => Self::AnyPointer,
            "Capability" => Self::Capability,
            "AnyStruct" => Self::AnyStruct,
            "AnyList" => Self::AnyList,
            "List" => Self::ListConstructor,
            _ => return None,
        })
    }

    // Pinned C++ gives built-ins IDs of 1000 + Declaration's union tag.
    // These IDs describe identifier targets; they are not schema nodes.
    fn identifier_id(&self, nodes: &[Node]) -> Option<u64> {
        Some(match self {
            Self::Void => 1014,
            Self::Bool => 1015,
            Self::Int8 => 1016,
            Self::Int16 => 1017,
            Self::Int32 => 1018,
            Self::Int64 => 1019,
            Self::UInt8 => 1020,
            Self::UInt16 => 1021,
            Self::UInt32 => 1022,
            Self::UInt64 => 1023,
            Self::Float32 => 1024,
            Self::Float64 => 1025,
            Self::Text => 1026,
            Self::Data => 1027,
            Self::ListConstructor | Self::List(_) => 1028,
            Self::AnyPointer => 1030,
            Self::AnyStruct => 1031,
            Self::AnyList => 1032,
            Self::Capability => 1033,
            Self::Namespace(index)
            | Self::Struct(index, _)
            | Self::Enum(index, _)
            | Self::Interface(index, _)
            | Self::Constant(index, _)
            | Self::Annotation(index, _) => nodes[*index].id,
            Self::Parameter(..) | Self::ImplicitParameter(_) => return None,
        })
    }

    pub fn emit(&self, mut builder: type_::Builder<'_>, nodes: &[Node]) {
        match self {
            Self::Void => builder.set_void(()),
            Self::Bool => builder.set_bool(()),
            Self::Int8 => builder.set_int8(()),
            Self::Int16 => builder.set_int16(()),
            Self::Int32 => builder.set_int32(()),
            Self::Int64 => builder.set_int64(()),
            Self::UInt8 => builder.set_uint8(()),
            Self::UInt16 => builder.set_uint16(()),
            Self::UInt32 => builder.set_uint32(()),
            Self::UInt64 => builder.set_uint64(()),
            Self::Float32 => builder.set_float32(()),
            Self::Float64 => builder.set_float64(()),
            Self::Text => builder.set_text(()),
            Self::Data => builder.set_data(()),
            Self::AnyPointer => builder
                .init_any_pointer()
                .init_unconstrained()
                .set_any_kind(()),
            Self::Capability => builder
                .init_any_pointer()
                .init_unconstrained()
                .set_capability(()),
            Self::AnyStruct => builder
                .init_any_pointer()
                .init_unconstrained()
                .set_struct(()),
            Self::AnyList => builder.init_any_pointer().init_unconstrained().set_list(()),
            Self::Parameter(scope, index) => {
                let mut out = builder.init_any_pointer().init_parameter();
                out.set_scope_id(nodes[*scope].id);
                out.set_parameter_index(*index);
            }
            Self::ImplicitParameter(index) => builder
                .init_any_pointer()
                .init_implicit_method_parameter()
                .set_parameter_index(*index),
            Self::Interface(index, brand) => {
                let mut out = builder.init_interface();
                out.set_type_id(nodes[*index].id);
                if !brand.0.is_empty() {
                    brand.emit(out.init_brand(), nodes);
                }
            }
            Self::List(element) => element.emit(builder.init_list().init_element_type(), nodes),
            Self::Enum(index, brand) => {
                let mut out = builder.init_enum();
                out.set_type_id(nodes[*index].id);
                if !brand.0.is_empty() {
                    brand.emit(out.init_brand(), nodes);
                }
            }
            Self::Struct(index, brand) => {
                let mut out = builder.init_struct();
                out.set_type_id(nodes[*index].id);
                if !brand.0.is_empty() {
                    brand.emit(out.init_brand(), nodes);
                }
            }
            Self::Namespace(_)
            | Self::ListConstructor
            | Self::Constant(..)
            | Self::Annotation(..) => {
                unreachable!("non-field type passed to emitter")
            }
        }
    }
}

pub(crate) enum Failure {
    Diagnostic(Diagnostic),
    Import {
        file: usize,
        index: usize,
    },
    Embed {
        file: usize,
        path: String,
        span: Span,
    },
}

impl From<Diagnostic> for Failure {
    fn from(value: Diagnostic) -> Self {
        Self::Diagnostic(value)
    }
}

#[derive(Clone, Copy)]
enum Binding {
    Node(usize),
    Alias(usize, usize),
    Parameter(usize, u16),
}

struct Resolver<'a> {
    graph: &'a Graph,
    implicit: Vec<String>,
    budget: &'a mut usize,
    // None marks an alias currently being resolved, for cycle detection.
    aliases: BTreeMap<(usize, usize), Option<Resolved>>,
    constants: BTreeMap<usize, Option<TypedValue>>,
    annotation_types: BTreeMap<usize, Option<Resolved>>,
    annotations: BTreeMap<usize, Vec<AppliedAnnotation>>,
    value_types: BTreeSet<usize>,
    identifiers: Vec<BTreeSet<(u32, u32, u64)>>,
    report_identifiers: bool,
}

impl Resolver<'_> {
    fn child(&self, scope: usize, name: &str) -> Option<Binding> {
        let node = &self.graph.nodes[scope];
        node.children
            .iter()
            .find(|&&i| self.graph.nodes[i].name == name)
            .map(|&i| Binding::Node(i))
            .or_else(|| {
                node.aliases
                    .iter()
                    .position(|a| a.name == name)
                    .map(|i| Binding::Alias(scope, i))
            })
    }

    fn lookup(&self, mut scope: usize, name: &str) -> Option<Binding> {
        loop {
            if let Some(binding) = self.child(scope, name) {
                return Some(binding);
            }
            if let Some(index) = self.graph.nodes[scope]
                .parameters
                .iter()
                .position(|p| p == name)
            {
                return Some(Binding::Parameter(scope, index as u16));
            }
            scope = self.graph.nodes[scope].parent?;
        }
    }

    fn binding(
        &mut self,
        binding: Binding,
        context: &Brand,
        depth: usize,
    ) -> Result<Resolved, Failure> {
        match binding {
            Binding::Parameter(scope, index) => Ok(Resolved::Parameter(scope, index)),
            Binding::Node(index) => Ok(match self.graph.nodes[index].kind {
                NodeKind::File => Resolved::Namespace(index),
                NodeKind::Struct(_) => {
                    Resolved::Struct(index, Brand::for_node(&self.graph.nodes, index, context))
                }
                NodeKind::Enum(_) => {
                    Resolved::Enum(index, Brand::for_node(&self.graph.nodes, index, context))
                }
                NodeKind::Interface { .. } => {
                    Resolved::Interface(index, Brand::for_node(&self.graph.nodes, index, context))
                }
                NodeKind::Const { .. } => {
                    Resolved::Constant(index, Brand::for_node(&self.graph.nodes, index, context))
                }
                NodeKind::Annotation { .. } => {
                    Resolved::Annotation(index, Brand::for_node(&self.graph.nodes, index, context))
                }
            }),
            Binding::Alias(scope, index) => {
                let alias = &self.graph.nodes[scope].aliases[index];
                if let Some(cached) = self.aliases.get(&(scope, index)) {
                    let mut value = cached.clone().ok_or_else(|| {
                        self.graph
                            .source(scope)
                            .error(alias.span, format!("cyclic alias '{}'", alias.name))
                    })?;
                    if let Some((_, brand)) = value.declaration_mut() {
                        if brand.0.is_empty() {
                            *brand = Brand::lexical(&self.graph.nodes, scope);
                        }
                    }
                    return value
                        .substitute(context, depth, self.budget)
                        .map_err(|m| self.graph.source(scope).error(alias.span, m).into());
                }
                self.aliases.insert((scope, index), None);
                // Method parameters are not in an alias declaration's lexical scope.
                let implicit = std::mem::take(&mut self.implicit);
                let value = self.resolve(scope, &alias.target, depth + 1);
                self.implicit = implicit;
                let value = value?;
                self.aliases.insert((scope, index), Some(value.clone()));
                let mut value = value;
                if let Some((_, brand)) = value.declaration_mut() {
                    if brand.0.is_empty() {
                        *brand = Brand::lexical(&self.graph.nodes, scope);
                    }
                }
                value
                    .substitute(context, depth, self.budget)
                    .map_err(|m| self.graph.source(scope).error(alias.span, m).into())
            }
        }
    }

    fn resolve(&mut self, scope: usize, ty: &Type, depth: usize) -> Result<Resolved, Failure> {
        let source = self.graph.source(scope);
        if *self.budget == 0 {
            return Err(source
                .error(ty.span, "type resolution work limit exceeded (1000000)")
                .into());
        }
        *self.budget -= 1;
        if depth >= 64 {
            return Err(source
                .error(ty.span, "type/alias resolution nesting limit exceeded (64)")
                .into());
        }
        let result = match &ty.kind {
            TypeKind::Name { name, absolute } => {
                if !absolute {
                    if let Some(index) = self.implicit.iter().position(|p| p == name) {
                        return Ok(Resolved::ImplicitParameter(index as u16));
                    }
                }
                let binding = if *absolute {
                    let root = self.graph.files[self.graph.nodes[scope].file].root;
                    self.child(root, name)
                } else {
                    self.lookup(scope, name)
                };
                match binding {
                    Some(binding) => {
                        self.binding(binding, &Brand::lexical(&self.graph.nodes, scope), depth)
                    }
                    None if !absolute => Resolved::builtin(name).ok_or_else(|| {
                        source
                            .error(ty.span, format!("unknown type '{name}'"))
                            .into()
                    }),
                    None => Err(source
                        .error(ty.span, format!("unknown type '.{name}'"))
                        .into()),
                }
            }
            TypeKind::Import(index) => {
                let file_index = self.graph.nodes[scope].file;
                let file = &self.graph.files[file_index];
                if file.imports[*index].target == usize::MAX {
                    return Err(Failure::Import {
                        file: file_index,
                        index: *index,
                    });
                }
                Ok(Resolved::Namespace(
                    self.graph.files[file.imports[*index].target].root,
                ))
            }
            TypeKind::Member(parent, name) => {
                let parent = self.resolve(scope, parent, depth + 1)?;
                let (parent, context) = match parent {
                    Resolved::Namespace(index) => (index, Brand::default()),
                    Resolved::Struct(index, brand)
                    | Resolved::Enum(index, brand)
                    | Resolved::Interface(index, brand) => (index, brand),
                    _ => {
                        return Err(source
                            .error(ty.span, "type has no nested declarations")
                            .into())
                    }
                };
                let binding = self.child(parent, name).ok_or_else(|| {
                    source.error(ty.span, format!("unknown nested type '{name}'"))
                })?;
                self.binding(binding, &context, depth)
            }
            TypeKind::Apply(constructor, arguments) => {
                let constructor = self.resolve(scope, constructor, depth + 1)?;
                if !matches!(constructor, Resolved::ListConstructor) {
                    let mut constructor = constructor;
                    let Some((index, brand)) = constructor.declaration_mut() else {
                        if arguments.is_empty() {
                            return Ok(constructor);
                        }
                        return Err(source
                            .error(ty.span, "type does not accept generic parameters")
                            .into());
                    };
                    if matches!(brand.get(index), Some(Scope::Bind(_))) {
                        return Err(source
                            .error(ty.span, "double application of generic parameters")
                            .into());
                    }
                    if arguments.len() != self.graph.nodes[index].parameters.len() {
                        return Err(source
                            .error(ty.span, "incorrect number of generic parameters")
                            .into());
                    }
                    let mut types = Vec::new();
                    for argument in arguments {
                        let value = self.field_type(scope, argument, depth + 1)?;
                        if !matches!(
                            value,
                            Resolved::Text
                                | Resolved::Data
                                | Resolved::AnyPointer
                                | Resolved::List(_)
                                | Resolved::Struct(..)
                                | Resolved::Interface(..)
                                | Resolved::Parameter(..)
                                | Resolved::ImplicitParameter(_)
                        ) {
                            return Err(source
                                .error(argument.span, "only pointer types can be generic arguments")
                                .into());
                        }
                        types.push(value);
                    }
                    if !types.is_empty() {
                        brand.bind(index, types);
                    }
                    return Ok(constructor);
                }
                if arguments.len() != 1 {
                    return Err(source
                        .error(ty.span, "List requires exactly one type argument")
                        .into());
                }
                let element = self.field_type(scope, &arguments[0], depth + 1)?;
                if matches!(
                    element,
                    Resolved::AnyPointer
                        | Resolved::AnyStruct
                        | Resolved::Parameter(..)
                        | Resolved::ImplicitParameter(_)
                ) {
                    return Err(source
                        .error(ty.span, "List(AnyPointer), List(AnyStruct) and lists of unbound parameters are not supported by this frontend")
                        .into());
                }
                let mut height = 1;
                let mut cursor = &element;
                while let Resolved::List(inner) = cursor {
                    height += 1;
                    cursor = inner;
                }
                if height >= 64 {
                    return Err(source
                        .error(ty.span, "expanded list nesting limit exceeded (64)")
                        .into());
                }
                Ok(Resolved::List(Box::new(element)))
            }
        }?;
        // Applications report their constructor and arguments, not a second
        // reference covering the entire application. Parameter names have no
        // declaration ID. Implicit stream signatures have no source expression.
        if self.report_identifiers && !matches!(ty.kind, TypeKind::Apply(..)) {
            if let Some(id) = result.identifier_id(&self.graph.nodes) {
                self.identifiers[self.graph.nodes[scope].file].insert((
                    ty.span.start as u32,
                    ty.span.end as u32,
                    id,
                ));
            }
        }
        Ok(result)
    }

    fn field_type(&mut self, scope: usize, ty: &Type, depth: usize) -> Result<Resolved, Failure> {
        let value = self.resolve(scope, ty, depth)?;
        value
            .check(depth, self.budget)
            .map_err(|m| self.graph.source(scope).error(ty.span, m))?;
        match value {
            Resolved::Namespace(_) => Err(self
                .graph
                .source(scope)
                .error(ty.span, "file is not a field type")
                .into()),
            Resolved::Constant(..) => Err(self
                .graph
                .source(scope)
                .error(ty.span, "constant is not a field type")
                .into()),
            Resolved::Annotation(..) => Err(self
                .graph
                .source(scope)
                .error(ty.span, "annotation is not a field type")
                .into()),
            Resolved::ListConstructor => Err(self
                .graph
                .source(scope)
                .error(ty.span, "List requires exactly one type argument")
                .into()),
            _ => Ok(value),
        }
    }
    fn constant(&mut self, index: usize, depth: usize) -> Result<TypedValue, Failure> {
        let node = &self.graph.nodes[index];
        if depth >= 64 {
            return Err(self
                .graph
                .source(index)
                .error(node.span, "constant evaluation nesting limit exceeded (64)")
                .into());
        }
        if let Some(cached) = self.constants.get(&index) {
            return cached.clone().ok_or_else(|| {
                self.graph
                    .source(index)
                    .error(node.span, format!("cyclic constant '{}'", node.name))
                    .into()
            });
        }
        self.constants.insert(index, None);
        let NodeKind::Const { ty, value } = &node.kind else {
            unreachable!()
        };
        let ty = self.field_type(index, ty, depth)?;
        let value = self.evaluate(index, Some(value), &ty, depth + 1)?;
        let result = TypedValue { ty, value };
        // Bootstrap constants are not available until their annotations finish.
        self.node_annotations(index, depth + 1)?;
        self.constants.insert(index, Some(result.clone()));
        Ok(result)
    }

    fn annotation_type(&mut self, index: usize, depth: usize) -> Result<Resolved, Failure> {
        let node = &self.graph.nodes[index];
        let error = |m| self.graph.source(index).error(node.span, m);
        if depth >= 64 {
            return Err(error("annotation evaluation nesting limit exceeded (64)").into());
        }
        if let Some(value) = self.annotation_types.get(&index) {
            return value
                .clone()
                .ok_or_else(|| error("cyclic annotation dependency").into());
        }
        self.annotation_types.insert(index, None);
        let NodeKind::Annotation { ty, .. } = &node.kind else {
            unreachable!()
        };
        let ty = self.field_type(index, ty, depth + 1)?;
        self.node_annotations(index, depth + 1)?;
        self.annotation_types.insert(index, Some(ty.clone()));
        Ok(ty)
    }

    fn node_annotations(
        &mut self,
        index: usize,
        depth: usize,
    ) -> Result<Vec<AppliedAnnotation>, Failure> {
        if let Some(annotations) = self.annotations.get(&index) {
            return Ok(annotations.clone());
        }
        let node = &self.graph.nodes[index];
        let target = match node.kind {
            NodeKind::File => AnnotationTarget::File,
            NodeKind::Const { .. } => AnnotationTarget::Const,
            NodeKind::Annotation { .. } => AnnotationTarget::Annotation,
            NodeKind::Enum(_) => AnnotationTarget::Enum,
            NodeKind::Interface { .. } => AnnotationTarget::Interface,
            NodeKind::Struct(_) => AnnotationTarget::Struct,
        };
        let annotations = self.applications(index, &node.annotations, target, depth)?;
        self.annotations.insert(index, annotations.clone());
        Ok(annotations)
    }

    fn applications(
        &mut self,
        scope: usize,
        annotations: &[Annotation],
        target: AnnotationTarget,
        depth: usize,
    ) -> Result<Vec<AppliedAnnotation>, Failure> {
        // C++ resolves applications in the enclosing interface, not in a
        // detached method struct's implicit parameter scope.
        let scope = if self.graph.nodes[scope].method.is_some() {
            self.graph.nodes[scope].parent.unwrap()
        } else {
            scope
        };
        let mut result = Vec::with_capacity(annotations.len());
        for annotation in annotations {
            let error = |m| self.graph.source(scope).error(annotation.span, m);
            let Resolved::Annotation(index, brand) =
                self.resolve(scope, &annotation.name, depth)?
            else {
                return Err(error("declaration is not an annotation").into());
            };
            let NodeKind::Annotation { targets, .. } = self.graph.nodes[index].kind else {
                unreachable!()
            };
            if targets & target.mask() == 0 {
                return Err(error(&format!(
                    "annotation cannot be applied to {} declarations",
                    target.name()
                ))
                .into());
            }
            let ty = self
                .annotation_type(index, depth + 1)?
                .substitute(&brand, depth, self.budget)
                .map_err(error)?;
            if annotation.value.is_none() && ty != Resolved::Void {
                return Err(error("annotation requires a value").into());
            }
            let value = self.evaluate(scope, annotation.value.as_ref(), &ty, depth + 1)?;
            self.value_types.insert(index);
            result.push(AppliedAnnotation {
                declaration: index,
                brand,
                value: TypedValue { ty, value },
                span: annotation.span,
            });
        }
        Ok(result)
    }

    fn evaluate(
        &mut self,
        scope: usize,
        expression: Option<&Expr>,
        ty: &Resolved,
        depth: usize,
    ) -> Result<Value, Failure> {
        let Some(expression) = expression else {
            return Ok(Value::implicit(ty));
        };
        let source = self.graph.source(scope);
        let error = |message| source.error(expression.span, message);
        if *self.budget == 0 {
            return Err(error("type/value resolution work limit exceeded (1000000)").into());
        }
        *self.budget -= 1;
        if depth >= 64 {
            return Err(error("constant evaluation nesting limit exceeded (64)").into());
        }
        match &expression.kind {
            ExprKind::Embed(path) => {
                let file = self.graph.nodes[scope].file;
                let bytes = self
                    .graph
                    .embeds
                    .get(&(file, path.clone()))
                    .ok_or_else(|| Failure::Embed {
                        file,
                        path: path.clone(),
                        span: expression.span,
                    })?;
                match ty {
                    Resolved::Text => Ok(Value::Text(Some(bytes.bytes.clone()))),
                    Resolved::Data => Ok(Value::Data(Some(bytes.bytes.clone()))),
                    Resolved::Struct(node, brand) => {
                        if self.graph.nodes[*node].is_group {
                            return Err(
                                error("embedded messages cannot be used as group values").into()
                            );
                        }
                        self.value_types.insert(*node);
                        Value::embedded(*node, brand.clone(), bytes).map_err(|e| {
                            source
                                .error(expression.span, format!("invalid embedded message: {e}"))
                                .into()
                        })
                    }
                    _ => Err(error("embeds require Text, Data or a struct type").into()),
                }
            }
            ExprKind::List(expressions) => {
                let Resolved::List(element) = ty else {
                    return Err(error("list literal requires a List field type").into());
                };
                dependency(element, &mut self.value_types);
                let values = expressions
                    .iter()
                    .map(|e| self.evaluate(scope, Some(e), element, depth + 1))
                    .collect::<Result<Vec<_>, _>>()?;
                Value::list((**element).clone(), values).map_err(|e| error(e).into())
            }
            ExprKind::Struct(assignments) => {
                let Resolved::Struct(node, brand) = ty else {
                    return Err(error("struct literal requires a struct field type").into());
                };
                self.value_types.insert(*node);
                let NodeKind::Struct(fields) = &self.graph.nodes[*node].kind else {
                    unreachable!()
                };
                let mut values = Vec::with_capacity(assignments.len());
                for assignment in assignments {
                    let index = fields
                        .iter()
                        .position(|f| f.name == assignment.name)
                        .ok_or_else(|| {
                            source.error(
                                assignment.span,
                                format!("struct has no field named '{}'", assignment.name),
                            )
                        })?;
                    let target = match &fields[index].kind {
                        FieldKind::Slot { ty, .. } => self
                            .field_type(*node, ty, depth + 1)?
                            .substitute(brand, depth, self.budget)
                            .map_err(error)?,
                        FieldKind::Group(group) => Resolved::Struct(*group, brand.clone()),
                    };
                    let value =
                        self.evaluate(scope, Some(&assignment.value), &target, depth + 1)?;
                    values.push((index, value));
                }
                Value::structure(*node, brand.clone(), values).map_err(|e| error(e).into())
            }
            ExprKind::Literal(token) => {
                let value = Value::literal(token, ty).map_err(error)?;
                self.coerce(scope, expression, value, ty, depth)
            }
            ExprKind::Reference(reference) => {
                if let TypeKind::Name {
                    name,
                    absolute: false,
                } = &reference.kind
                {
                    if let Some(value) = Value::name(name, ty, &self.graph.nodes) {
                        return self.coerce(scope, expression, value, ty, depth);
                    }
                }
                let target = self.resolve(scope, reference, depth)?;
                let Resolved::Constant(index, brand) = target else {
                    return Err(error("value expression does not refer to a constant").into());
                };
                if matches!(
                    reference.kind,
                    TypeKind::Name {
                        absolute: false,
                        ..
                    }
                ) {
                    return Err(error(
                        "constant names must be qualified (for example .name or Scope.name)",
                    )
                    .into());
                }
                let constant = self.constant(index, depth + 1)?;
                let constant = constant
                    .substitute(&brand, depth, self.budget)
                    .map_err(error)?;
                if matches!(
                    constant.ty,
                    Resolved::AnyPointer
                        | Resolved::AnyStruct
                        | Resolved::AnyList
                        | Resolved::Capability
                        | Resolved::Parameter(..)
                        | Resolved::ImplicitParameter(_)
                ) {
                    return Err(error("references to AnyPointer constants are not supported by the pinned C++ compiler").into());
                }
                self.coerce(
                    scope,
                    expression,
                    constant.value.constant_reference(),
                    ty,
                    depth,
                )
            }
        }
    }
    fn coerce(
        &mut self,
        scope: usize,
        expression: &Expr,
        value: Value,
        ty: &Resolved,
        depth: usize,
    ) -> Result<Value, Failure> {
        match value.clone().coerce(ty) {
            Ok(value) => Ok(value),
            Err(message) => {
                let error = |m| self.graph.source(scope).error(expression.span, m);
                // C++ permits one implicit wrapping level, using an already
                // unambiguous value and the first field's type, not its name.
                if let Resolved::Struct(node, brand) = ty {
                    let NodeKind::Struct(fields) = &self.graph.nodes[*node].kind else {
                        unreachable!()
                    };
                    if let Some(field) = fields.first() {
                        let first = match &field.kind {
                            FieldKind::Slot { ty, .. } => self
                                .field_type(*node, ty, depth + 1)?
                                .substitute(brand, depth, self.budget)
                                .map_err(error)?,
                            FieldKind::Group(group) => Resolved::Struct(*group, brand.clone()),
                        };
                        if let Ok(value) = value.coerce(&first) {
                            self.value_types.insert(*node);
                            return Value::structure(*node, brand.clone(), vec![(0, value)])
                                .map_err(|e| error(e).into());
                        }
                    }
                }
                Err(error(message).into())
            }
        }
    }
}

#[derive(Clone)]
pub(crate) struct AppliedAnnotation {
    pub declaration: usize,
    pub brand: Brand,
    pub value: TypedValue,
    pub span: Span,
}

#[derive(Clone)]
pub(crate) struct BrandedNode {
    pub index: usize,
    pub brand: Brand,
}

#[derive(Clone)]
pub(crate) struct CompiledInterface {
    pub superclasses: Vec<BrandedNode>,
    pub methods: Vec<(BrandedNode, BrandedNode)>,
}

pub(crate) struct Compilation {
    pub interfaces: Vec<Option<CompiledInterface>>,
    pub selected: BTreeSet<usize>,
    pub required: BTreeSet<usize>,
    pub fields: Vec<Option<Vec<Option<TypedValue>>>>,
    pub constants: Vec<Option<TypedValue>>,
    pub annotation_types: Vec<Option<Resolved>>,
    pub annotations: Vec<Vec<AppliedAnnotation>>,
    pub member_annotations: Vec<Vec<Vec<AppliedAnnotation>>>,
    // Sorted, unique (start byte, end byte, target ID) entries per source file.
    pub identifiers: Vec<BTreeSet<(u32, u32, u64)>>,
}

pub(crate) fn compile(
    graph: &Graph,
    budget: &mut usize,
    additional: &BTreeSet<usize>,
) -> Result<Compilation, Failure> {
    let resolver = resolver(graph, budget, true);
    compile_selection(graph, resolver, additional)
}

fn resolver<'a>(graph: &'a Graph, budget: &'a mut usize, report_identifiers: bool) -> Resolver<'a> {
    Resolver {
        graph,
        implicit: vec![],
        budget,
        aliases: BTreeMap::new(),
        constants: BTreeMap::new(),
        annotation_types: BTreeMap::new(),
        annotations: BTreeMap::new(),
        value_types: BTreeSet::new(),
        identifiers: vec![BTreeSet::new(); graph.files.len()],
        report_identifiers,
    }
}

/// C++ Compiler::lookup() returns the declaration ID and discards its brand.
/// Aliases of parameters have no schema; built-in types have no loadable node.
pub(crate) fn lookup(
    graph: &Graph,
    scope: usize,
    name: &str,
    budget: &mut usize,
) -> Result<Option<usize>, Failure> {
    let mut resolver = resolver(graph, budget, false);
    let Some(binding) = resolver.child(scope, name) else {
        return Ok(None);
    };
    let value = resolver.binding(binding, &Brand::lexical(&graph.nodes, scope), 0)?;
    match value {
        Resolved::Namespace(index)
        | Resolved::Struct(index, _)
        | Resolved::Enum(index, _)
        | Resolved::Interface(index, _)
        | Resolved::Constant(index, _)
        | Resolved::Annotation(index, _) => Ok(Some(index)),
        Resolved::Parameter(..) | Resolved::ImplicitParameter(_) => Ok(None),
        _ => Err(graph
            .source(scope)
            .error(
                graph.nodes[scope].span,
                "nested alias does not name a loadable schema declaration",
            )
            .into()),
    }
}

fn compile_selection(
    graph: &Graph,
    mut resolver: Resolver<'_>,
    additional: &BTreeSet<usize>,
) -> Result<Compilation, Failure> {
    let mut result = Compilation {
        interfaces: vec![None; graph.nodes.len()],
        selected: BTreeSet::new(),
        required: BTreeSet::new(),
        fields: vec![None; graph.nodes.len()],
        constants: vec![None; graph.nodes.len()],
        annotation_types: vec![None; graph.nodes.len()],
        annotations: vec![vec![]; graph.nodes.len()],
        member_annotations: vec![vec![]; graph.nodes.len()],
        identifiers: vec![],
    };
    // A stack reproduces C++'s dependency-first traversal without recursing on
    // Rust's call stack. Aliases are checked after a requested scope's children.
    enum Work {
        Node(usize),
        Alias(usize, usize),
    }
    let mut work: Vec<_> = additional.iter().rev().copied().map(Work::Node).collect();
    work.extend(
        graph
            .requested
            .iter()
            .rev()
            .map(|&file| Work::Node(graph.files[file].root)),
    );
    let mut value_bytes = 0usize;
    let mut record = |index: usize, value: &TypedValue| -> Result<(), Failure> {
        value_bytes += value.value.pointer_bytes();
        if value_bytes > 16 * 1024 * 1024 {
            return Err(graph
                .source(index)
                .error(
                    graph.nodes[index].span,
                    "expanded value size limit exceeded (16 MiB)",
                )
                .into());
        }
        Ok(())
    };
    while let Some(item) = work
        .pop()
        .or_else(|| resolver.value_types.pop_first().map(Work::Node))
    {
        let index = match item {
            Work::Alias(scope, alias) => {
                resolver.binding(
                    Binding::Alias(scope, alias),
                    &Brand::lexical(&graph.nodes, scope),
                    0,
                )?;
                continue;
            }
            Work::Node(index) => index,
        };
        if !result.required.insert(index) {
            continue;
        }
        let node = &graph.nodes[index];
        let mut dependencies = Vec::new();
        let mut auxiliary = Vec::new();
        let mut deferred = Vec::new();
        if let NodeKind::Struct(fields) = &node.kind {
            compile_struct(
                index,
                &mut resolver,
                &mut result,
                &mut deferred,
                &mut record,
            )?;
            for position in 0..fields.len() {
                if let Some(value) = &result.fields[index].as_ref().unwrap()[position] {
                    dependency(&value.ty, &mut dependencies);
                }
                dependencies.extend(
                    result.member_annotations[index][position]
                        .iter()
                        .map(|a| a.declaration),
                );
            }
            if !node.is_group {
                auxiliary.extend(struct_groups(graph, index));
            }
        } else if let NodeKind::Interface {
            methods,
            superclasses,
        } = &node.kind
        {
            let mut interface = CompiledInterface {
                superclasses: vec![],
                methods: vec![],
            };
            for superclass in superclasses {
                let Resolved::Interface(base, brand) = resolver.field_type(index, superclass, 0)?
                else {
                    return Err(graph
                        .source(index)
                        .error(superclass.span, "superclass must be an interface")
                        .into());
                };
                dependency(&Resolved::Interface(base, brand.clone()), &mut dependencies);
                interface
                    .superclasses
                    .push(BrandedNode { index: base, brand });
            }
            for method in methods {
                resolver.implicit = method.parameters.clone();
                let mut signature = Vec::with_capacity(2);
                for params in [&method.params, &method.results] {
                    let target = match params {
                        ParamList::Inline(target) => {
                            // Inline signatures are translated as part of the interface,
                            // before the next signature or method is resolved. Their type
                            // parameters are ordinary parameters of the detached struct.
                            let implicit = std::mem::take(&mut resolver.implicit);
                            compile_struct(
                                *target,
                                &mut resolver,
                                &mut result,
                                &mut deferred,
                                &mut record,
                            )?;
                            resolver.implicit = implicit;
                            auxiliary.push(*target);
                            BrandedNode {
                                index: *target,
                                brand: Brand::lexical(&graph.nodes, index),
                            }
                        }
                        ParamList::Type(ty) | ParamList::Stream(ty) => {
                            resolver.report_identifiers = !matches!(params, ParamList::Stream(_));
                            let resolved = resolver.field_type(index, ty, 0);
                            resolver.report_identifiers = true;
                            let Resolved::Struct(target, brand) = resolved? else {
                                return Err(graph
                                    .source(index)
                                    .error(ty.span, "method signature must be a struct type")
                                    .into());
                            };
                            if matches!(params, ParamList::Stream(_))
                                && graph.nodes[target].id != 0x995f9a3377c0b16e
                            {
                                return Err(graph
                                    .source(index)
                                    .error(
                                        ty.span,
                                        "stream requires the standard StreamResult schema ID",
                                    )
                                    .into());
                            }
                            BrandedNode {
                                index: target,
                                brand,
                            }
                        }
                    };
                    if !matches!(params, ParamList::Inline(_)) {
                        dependency(
                            &Resolved::Struct(target.index, target.brand.clone()),
                            &mut dependencies,
                        );
                    }
                    signature.push(target);
                }
                resolver.implicit.clear();
                let results = signature.pop().unwrap();
                let params = signature.pop().unwrap();
                interface.methods.push((params, results));
                result.member_annotations[index].push(resolver.applications(
                    index,
                    &method.annotations,
                    AnnotationTarget::Method,
                    0,
                )?);
                dependencies.extend(
                    result.member_annotations[index]
                        .last()
                        .unwrap()
                        .iter()
                        .map(|a| a.declaration),
                );
            }
            result.interfaces[index] = Some(interface);
        } else if matches!(node.kind, NodeKind::Const { .. }) {
            let value = resolver.constant(index, 0)?;
            dependency(&value.ty, &mut dependencies);
            record(index, &value)?;
            result.constants[index] = Some(value);
        } else if matches!(node.kind, NodeKind::Annotation { .. }) {
            let ty = resolver.annotation_type(index, 0)?;
            dependency(&ty, &mut dependencies);
            result.annotation_types[index] = Some(ty);
        } else if let NodeKind::Enum(enumerants) = &node.kind {
            for enumerant in enumerants {
                result.member_annotations[index].push(resolver.applications(
                    index,
                    &enumerant.annotations,
                    AnnotationTarget::Enumerant,
                    0,
                )?);
            }
        }
        result.annotations[index] = resolver.node_annotations(index, 0)?;
        finish_defaults(&mut resolver, &mut result, deferred, &mut record)?;
        if matches!(node.kind, NodeKind::Enum(_)) {
            dependencies.extend(
                result.member_annotations[index]
                    .iter()
                    .flatten()
                    .map(|a| a.declaration),
            );
        }
        dependencies.extend(result.annotations[index].iter().map(|a| a.declaration));
        for annotation in &result.annotations[index] {
            record(index, &annotation.value)?;
        }
        if !matches!(node.kind, NodeKind::Struct(_)) {
            for annotation in result.member_annotations[index].iter().flatten() {
                record(index, &annotation.value)?;
            }
        }
        dependencies.extend(auxiliary);
        if graph.requested.contains(&node.file) {
            let mut aliases: Vec<_> = (0..node.aliases.len()).collect();
            aliases.sort_by(|&a, &b| node.aliases[a].name.cmp(&node.aliases[b].name));
            work.extend(
                aliases
                    .into_iter()
                    .rev()
                    .map(|alias| Work::Alias(index, alias)),
            );
            work.extend(node.children.iter().rev().copied().map(Work::Node));
        }
        if let Some(parent) = node.parent {
            work.push(Work::Node(parent));
        }
        work.extend(dependencies.into_iter().rev().map(Work::Node));
    }
    let mut todo = BTreeSet::new();
    // Only type and annotation dependencies belong in the request. Types needed to encode an
    // erased/imported constant still get checked and laid out, without leaking
    // their schema nodes into generated-code dependencies.
    todo.extend(
        graph
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| graph.requested.contains(&n.file))
            .map(|(i, _)| i),
    );
    todo.extend(additional);
    while let Some(index) = todo.pop_first() {
        if !result.selected.insert(index) {
            continue;
        }
        if let Some(parent) = graph.nodes[index].parent {
            todo.insert(parent);
        }
        if let Some(fields) = &result.fields[index] {
            for field in fields.iter().flatten() {
                dependency(&field.ty, &mut todo);
            }
            let NodeKind::Struct(fields) = &graph.nodes[index].kind else {
                unreachable!()
            };
            for field in fields {
                if let FieldKind::Group(group) = field.kind {
                    todo.insert(group);
                }
            }
        }
        if let Some(interface) = &result.interfaces[index] {
            for node in interface
                .superclasses
                .iter()
                .chain(interface.methods.iter().flat_map(|(p, r)| [p, r]))
            {
                dependency(&Resolved::Struct(node.index, node.brand.clone()), &mut todo);
            }
        }
        if let Some(constant) = &result.constants[index] {
            dependency(&constant.ty, &mut todo);
        }
        if let Some(ty) = &result.annotation_types[index] {
            dependency(ty, &mut todo);
        }
        for annotation in result.annotations[index]
            .iter()
            .chain(result.member_annotations[index].iter().flatten())
        {
            todo.insert(annotation.declaration);
        }
    }
    result.identifiers = resolver.identifiers;
    Ok(result)
}

// Auxiliary group schemas are traversed in declaration order, independently
// of the field ordinals that determine their position in the parent schema.
fn struct_groups(graph: &Graph, index: usize) -> Vec<usize> {
    let mut pending = vec![index];
    let mut groups = Vec::new();
    while let Some(index) = pending.pop() {
        let NodeKind::Struct(fields) = &graph.nodes[index].kind else {
            unreachable!()
        };
        for field in fields {
            if let FieldKind::Group(group) = field.kind {
                groups.push(group);
                pending.push(group);
            }
        }
    }
    groups.sort_by_key(|&index| graph.nodes[index].span.start);
    groups
}

// Translate a struct and all of its groups together. C++ resolves every slot in
// the shared ordinal namespace before applying member annotations in source
// order. Composite defaults wait until the enclosing node's bootstrap finishes.
fn compile_struct(
    mut index: usize,
    resolver: &mut Resolver<'_>,
    result: &mut Compilation,
    deferred: &mut Vec<(usize, usize)>,
    record: &mut impl FnMut(usize, &TypedValue) -> Result<(), Failure>,
) -> Result<(), Failure> {
    let graph = resolver.graph;
    while graph.nodes[index].is_group {
        index = graph.nodes[index].parent.unwrap();
    }
    if result.fields[index].is_some() {
        return Ok(());
    }
    let mut members = Vec::new();
    for group in std::iter::once(index).chain(struct_groups(graph, index)) {
        let NodeKind::Struct(fields) = &graph.nodes[group].kind else {
            unreachable!()
        };
        result.fields[group] = Some(vec![None; fields.len()]);
        result.member_annotations[group] = vec![vec![]; fields.len()];
        for (position, field) in fields.iter().enumerate() {
            members.push((group, position, field));
        }
    }
    let mut slots: Vec<_> = members
        .iter()
        .filter(|(_, _, field)| matches!(field.kind, FieldKind::Slot { .. }))
        .copied()
        .collect();
    slots.sort_by_key(|(_, _, field)| field.ordinal);
    for (group, position, field) in slots {
        let FieldKind::Slot { ty, default } = &field.kind else {
            unreachable!()
        };
        let ty = resolver.field_type(group, ty, 0)?;
        let value = if matches!(field.annotation_target, AnnotationTarget::Param)
            && matches!(default.as_ref().map(|e| &e.kind), Some(ExprKind::Reference(Type { kind: TypeKind::Name { name, absolute: false }, .. })) if name == "null")
        {
            if ty.data_bits().is_some() {
                return Err(graph
                    .source(group)
                    .error(field.span, "only pointer parameters may default to null")
                    .into());
            }
            Value::implicit(&ty)
        } else if matches!(
            ty,
            Resolved::List(_)
                | Resolved::Struct(..)
                | Resolved::Interface(..)
                | Resolved::AnyPointer
                | Resolved::AnyStruct
                | Resolved::AnyList
                | Resolved::Capability
                | Resolved::Parameter(..)
                | Resolved::ImplicitParameter(_)
        ) && default.is_some()
        {
            deferred.push((group, position));
            Value::implicit(&ty)
        } else {
            resolver.evaluate(value_scope(graph, group), default.as_ref(), &ty, 0)?
        };
        let value = TypedValue { ty, value };
        // Check expanded bytes as each value is built, including inline
        // signatures and groups; do not accumulate an entire node first.
        record(group, &value)?;
        result.fields[group].as_mut().unwrap()[position] = Some(value);
    }
    members.sort_by_key(|(_, _, field)| field.span.start);
    for (group, position, field) in members {
        let annotations =
            resolver.applications(group, &field.annotations, field.annotation_target, 0)?;
        for annotation in &annotations {
            record(group, &annotation.value)?;
        }
        result.member_annotations[group][position] = annotations;
    }
    Ok(())
}

fn value_scope(graph: &Graph, index: usize) -> usize {
    if graph.nodes[index].method.is_some() {
        graph.nodes[index].parent.unwrap()
    } else {
        index
    }
}

fn finish_defaults(
    resolver: &mut Resolver<'_>,
    result: &mut Compilation,
    deferred: Vec<(usize, usize)>,
    record: &mut impl FnMut(usize, &TypedValue) -> Result<(), Failure>,
) -> Result<(), Failure> {
    for (index, position) in deferred {
        let NodeKind::Struct(fields) = &resolver.graph.nodes[index].kind else {
            unreachable!()
        };
        let FieldKind::Slot { default, .. } = &fields[position].kind else {
            unreachable!()
        };
        let value = result.fields[index].as_mut().unwrap()[position]
            .as_mut()
            .unwrap();
        value.value = resolver.evaluate(
            value_scope(resolver.graph, index),
            default.as_ref(),
            &value.ty,
            0,
        )?;
        record(index, value)?;
    }
    Ok(())
}

fn dependency(ty: &Resolved, todo: &mut impl Extend<usize>) {
    match ty {
        Resolved::List(element) => dependency(element, todo),
        Resolved::Enum(target, brand)
        | Resolved::Struct(target, brand)
        | Resolved::Interface(target, brand) => {
            todo.extend([*target]);
            for (_, scope) in &brand.0 {
                if let Scope::Bind(types) = scope {
                    for ty in types.iter() {
                        dependency(ty, todo);
                    }
                }
            }
        }
        _ => (),
    }
}
