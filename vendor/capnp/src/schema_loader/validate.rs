use super::*;
use crate::schema_capnp::value;
use alloc::collections::BTreeSet;
pub(super) struct Requirement {
    pub kind: Kind,
    pub data: u16,
    pub pointers: u16,
}
type Requirements = BTreeMap<u64, Requirement>;
fn dependency(req: &mut Requirements, id: u64, kind: Kind) -> Result<()> {
    require(id != 0, "zero schema dependency ID")?;
    if let Some(old) = req.get(&id) {
        require(old.kind == kind, "conflicting dependency kinds")?;
    } else {
        req.insert(
            id,
            Requirement {
                kind,
                data: 0,
                pointers: 0,
            },
        );
    }
    Ok(())
}
fn names<'a>(
    members: impl Iterator<Item = (Result<crate::text::Reader<'a>>, u16)>,
    len: u32,
) -> Result<()> {
    require(len <= u16::MAX as u32, "too many schema members")?;
    let mut names = BTreeSet::new();
    let mut orders = BTreeSet::new();
    for (name, order) in members {
        require(
            names.insert(name?.to_str()?) && u32::from(order) < len && orders.insert(order),
            "duplicate name or invalid code order",
        )?;
    }
    Ok(())
}
pub(super) fn node(n: node::Reader<'_>, req: &mut Requirements) -> Result<()> {
    require(n.get_id() != 0, "zero schema ID")?;
    n.get_display_name()?.to_str()?;
    require(
        n.get_parameters()?.len() <= u16::MAX as u32,
        "too many type parameters",
    )?;
    match n.which()? {
        node::Struct(s) => {
            let fields = s.get_fields()?;
            names(
                fields.iter().map(|f| (f.get_name(), f.get_code_order())),
                fields.len(),
            )?;
            let count = s.get_discriminant_count();
            require(
                count != 1 && u32::from(count) <= fields.len(),
                "invalid union member count",
            )?;
            require(
                count == 0
                    || (u64::from(s.get_discriminant_offset()) + 1) * 16
                        <= u64::from(s.get_data_word_count()) * 64,
                "union discriminant out of bounds",
            )?;
            let mut discriminants = BTreeSet::new();
            let mut ordinal = 0u32;
            for f in fields {
                if let field::ordinal::Explicit(o) = f.get_ordinal().which()? {
                    require(u32::from(o) >= ordinal, "fields not ordered by ordinal")?;
                    ordinal = u32::from(o) + 1;
                }
                let d = f.get_discriminant_value();
                if d != field::NO_DISCRIMINANT {
                    require(
                        d < count && discriminants.insert(d),
                        "invalid union discriminant",
                    )?;
                }
                match f.which()? {
                    field::Slot(slot) => {
                        let t = slot.get_type()?;
                        ty(t, req, 0)?;
                        default(t, slot.get_default_value()?)?;
                        let (bits, pointer) = storage(t)?;
                        let end = u64::from(slot.get_offset()) + 1;
                        require(
                            end * bits <= u64::from(s.get_data_word_count()) * 64
                                && (!pointer || end <= u64::from(s.get_pointer_count())),
                            "field offset out of bounds",
                        )?;
                    }
                    field::Group(g) => dependency(req, g.get_type_id(), Kind::Struct)?,
                }
            }
            require(
                discriminants.len() == usize::from(count),
                "union discriminant count mismatch",
            )?;
            if s.get_is_group() {
                dependency(req, n.get_scope_id(), Kind::Struct)?;
                let r = req.get_mut(&n.get_scope_id()).unwrap();
                r.data = r.data.max(s.get_data_word_count());
                r.pointers = r.pointers.max(s.get_pointer_count());
            }
        }
        node::Enum(s) => {
            let fields = s.get_enumerants()?;
            names(
                fields.iter().map(|e| (e.get_name(), e.get_code_order())),
                fields.len(),
            )?;
        }
        node::Interface(s) => {
            for parent in s.get_superclasses()? {
                dependency(req, parent.get_id(), Kind::Interface)?;
                brand(parent.get_brand()?, req, 0)?;
            }
            let methods = s.get_methods()?;
            names(
                methods.iter().map(|m| (m.get_name(), m.get_code_order())),
                methods.len(),
            )?;
            for m in methods {
                dependency(req, m.get_param_struct_type(), Kind::Struct)?;
                dependency(req, m.get_result_struct_type(), Kind::Struct)?;
                brand(m.get_param_brand()?, req, 0)?;
                brand(m.get_result_brand()?, req, 0)?;
            }
        }
        node::Const(c) => {
            ty(c.get_type()?, req, 0)?;
            default(c.get_type()?, c.get_value()?)?;
        }
        node::Annotation(a) => ty(a.get_type()?, req, 0)?,
        node::File(()) => (),
    }
    Ok(())
}
fn ty(t: type_::Reader<'_>, req: &mut Requirements, depth: usize) -> Result<()> {
    require(depth < 64, "schema type depth limit")?;
    let Ok(which) = t.which() else {
        return Ok(());
    };
    match which {
        type_::Struct(t) => {
            dependency(req, t.get_type_id(), Kind::Struct)?;
            brand(t.get_brand()?, req, depth + 1)?;
        }
        type_::Enum(t) => {
            dependency(req, t.get_type_id(), Kind::Enum)?;
            brand(t.get_brand()?, req, depth + 1)?;
        }
        type_::Interface(t) => {
            dependency(req, t.get_type_id(), Kind::Interface)?;
            brand(t.get_brand()?, req, depth + 1)?;
        }
        type_::List(l) => ty(l.get_element_type()?, req, depth + 1)?,
        type_::AnyPointer(p) => {
            if let type_::any_pointer::Unconstrained(p) = p.which()? {
                p.which()?;
            }
        }
        _ => (),
    }
    Ok(())
}
fn brand(b: brand::Reader<'_>, req: &mut Requirements, depth: usize) -> Result<()> {
    require(depth < 64, "schema brand depth limit")?;
    let mut scopes = BTreeSet::new();
    for s in b.get_scopes()? {
        require(
            s.get_scope_id() != 0 && scopes.insert(s.get_scope_id()),
            "duplicate or zero brand scope",
        )?;
        if let brand::scope::Bind(bindings) = s.which()? {
            for b in bindings? {
                if let brand::binding::Type(t) = b.which()? {
                    let t = t?;
                    ty(t, req, depth + 1)?;
                    require(storage(t)?.1, "generic argument must be a pointer")?;
                }
            }
        }
    }
    Ok(())
}
pub(super) fn storage(t: type_::Reader<'_>) -> Result<(u64, bool)> {
    let Ok(which) = t.which() else {
        return Ok((0, false));
    };
    Ok(match which {
        type_::Void(()) => (0, false),
        type_::Bool(()) => (1, false),
        type_::Int8(()) | type_::Uint8(()) => (8, false),
        type_::Int16(()) | type_::Uint16(()) | type_::Enum(_) => (16, false),
        type_::Int32(()) | type_::Uint32(()) | type_::Float32(()) => (32, false),
        type_::Int64(()) | type_::Uint64(()) | type_::Float64(()) => (64, false),
        _ => (0, true),
    })
}
fn default(t: type_::Reader<'_>, v: value::Reader<'_>) -> Result<()> {
    let Ok(which) = t.which() else {
        return Ok(());
    };
    let ok = matches!(
        (which, v.which()?),
        (type_::Void(_), value::Void(_))
            | (type_::Bool(_), value::Bool(_))
            | (type_::Int8(_), value::Int8(_))
            | (type_::Int16(_), value::Int16(_))
            | (type_::Int32(_), value::Int32(_))
            | (type_::Int64(_), value::Int64(_))
            | (type_::Uint8(_), value::Uint8(_))
            | (type_::Uint16(_), value::Uint16(_))
            | (type_::Uint32(_), value::Uint32(_))
            | (type_::Uint64(_), value::Uint64(_))
            | (type_::Float32(_), value::Float32(_))
            | (type_::Float64(_), value::Float64(_))
            | (type_::Text(_), value::Text(_))
            | (type_::Data(_), value::Data(_))
            | (type_::List(_), value::List(_))
            | (type_::Enum(_), value::Enum(_))
            | (type_::Struct(_), value::Struct(_))
            | (type_::Interface(_), value::Interface(_))
            | (type_::AnyPointer(_), value::AnyPointer(_))
    );
    require(ok, "field type and default mismatch")
}
/// Reject group/inheritance cycles before any recursive dynamic operation.
pub(super) fn graph(loader: &SchemaLoader) -> Result<()> {
    fn visit(
        loader: &SchemaLoader,
        id: u64,
        path: &mut BTreeSet<u64>,
        done: &mut BTreeSet<u64>,
    ) -> Result<()> {
        if done.contains(&id) {
            return Ok(());
        }
        require(
            path.len() < 64 && path.insert(id),
            "cyclic or overdeep group/interface graph",
        )?;
        let n = loader.entries[&id].proto();
        match n.which()? {
            node::Struct(s) => {
                for f in s.get_fields()? {
                    if let field::Group(g) = f.which()? {
                        let child = &loader.entries[&g.get_type_id()];
                        if !child.stub {
                            let node::Struct(c) = child.proto().which()? else {
                                unreachable!()
                            };
                            require(
                                c.get_is_group() && child.proto().get_scope_id() == id,
                                "group has wrong parent",
                            )?;
                            require(
                                c.get_data_word_count() <= s.get_data_word_count()
                                    && c.get_pointer_count() <= s.get_pointer_count(),
                                "group exceeds parent size",
                            )?;
                        }
                        visit(loader, g.get_type_id(), path, done)?;
                    }
                }
            }
            node::Interface(s) => {
                for parent in s.get_superclasses()? {
                    visit(loader, parent.get_id(), path, done)?;
                }
            }
            _ => (),
        }
        path.remove(&id);
        done.insert(id);
        Ok(())
    }
    let mut done = BTreeSet::new();
    for &id in loader.entries.keys() {
        visit(loader, id, &mut BTreeSet::new(), &mut done)?;
    }
    Ok(())
}

// A group loaded independently can enlarge its containing group, which in turn
// enlarges the outer struct. Iterate to a fixed point before exposing builders.
pub(super) fn group_sizes(loader: &mut SchemaLoader) -> Result<()> {
    for _ in 0..64 {
        let mut requirements = BTreeMap::<u64, (u16, u16)>::new();
        for entry in loader.entries.values() {
            let n = entry.proto();
            if let node::Struct(s) = n.which()? {
                if s.get_is_group() {
                    let r = requirements.entry(n.get_scope_id()).or_default();
                    r.0 = r.0.max(s.get_data_word_count());
                    r.1 = r.1.max(s.get_pointer_count());
                }
            }
        }
        let mut changed = false;
        for (id, (data, pointers)) in requirements {
            let entry = &loader.entries[&id];
            let node::Struct(s) = entry.proto().which()? else {
                return Err(invalid("group parent is not a struct"));
            };
            let data = data.max(s.get_data_word_count());
            let pointers = pointers.max(s.get_pointer_count());
            if data != s.get_data_word_count() || pointers != s.get_pointer_count() {
                let mut message = message::Builder::new_default();
                message.set_root(entry.proto())?;
                let node::Struct(mut s) = message.get_root::<node::Builder>()?.which()? else {
                    unreachable!()
                };
                s.set_data_word_count(data);
                s.set_pointer_count(pointers);
                loader.entries.insert(
                    id,
                    Entry {
                        message: Rc::new(message),
                        stub: entry.stub,
                    },
                );
                changed = true;
            }
        }
        if !changed {
            return Ok(());
        }
    }
    Err(invalid("group layout depth limit"))
}
