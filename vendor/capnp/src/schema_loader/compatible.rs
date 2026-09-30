use super::*;
use crate::schema_capnp::value;
use core::cmp::Ordering;
struct Direction(Ordering);
impl Direction {
    fn compare<T: Ord>(&mut self, old: T, new: T) -> Result<()> {
        let direction = new.cmp(&old);
        if direction != Ordering::Equal {
            require(
                self.0 == Ordering::Equal || self.0 == direction,
                "schema mixes upgrades and downgrades",
            )?;
            self.0 = direction;
        }
        Ok(())
    }
}
pub(super) fn replace(
    old: node::Reader<'_>,
    new: node::Reader<'_>,
    expectations: &mut Vec<Entry>,
) -> Result<bool> {
    require(kind(old)? == kind(new)?, "schema declaration kind changed")?;
    let mut d = Direction(Ordering::Equal);
    d.compare(old.get_parameters()?.len(), new.get_parameters()?.len())?;
    match (old.which()?, new.which()?) {
        (node::Struct(a), node::Struct(b)) => {
            d.compare(a.get_data_word_count(), b.get_data_word_count())?;
            d.compare(a.get_pointer_count(), b.get_pointer_count())?;
            d.compare(a.get_discriminant_count(), b.get_discriminant_count())?;
            if a.get_discriminant_count() > 0 && b.get_discriminant_count() > 0 {
                require(
                    a.get_discriminant_offset() == b.get_discriminant_offset(),
                    "union discriminant moved",
                )?;
            }
            let af = a.get_fields()?;
            let bf = b.get_fields()?;
            d.compare(af.len(), bf.len())?;
            for (a, b) in af.iter().zip(bf) {
                let discr = |f: field::Reader<'_>| {
                    if f.get_discriminant_value() == field::NO_DISCRIMINANT {
                        0
                    } else {
                        f.get_discriminant_value()
                    }
                };
                require(discr(a) == discr(b), "field discriminant changed")?;
                match (a.which()?, b.which()?) {
                    (field::Slot(a), field::Slot(b)) => {
                        require(a.get_offset() == b.get_offset(), "field offset changed")?;
                        let a_type = a.get_type()?;
                        types(a_type, b.get_type()?, &mut d, expectations, false, 0)?;
                        // Validation accepts future types with opaque defaults.
                        // Matching unknown types must remain reloadable even if
                        // their default's union tag is also from a future schema.
                        if a_type.which().is_ok() {
                            defaults(a.get_default_value()?, b.get_default_value()?)?;
                        }
                    }
                    (field::Group(a), field::Group(b)) => {
                        require(a.get_type_id() == b.get_type_id(), "group ID changed")?
                    }
                    (field::Slot(slot), field::Group(group)) => expectations.push(upgrade(
                        slot.get_type()?,
                        group.get_type_id(),
                        Some((old, a)),
                    )?),
                    (field::Group(group), field::Slot(slot)) => expectations.push(upgrade(
                        slot.get_type()?,
                        group.get_type_id(),
                        Some((new, b)),
                    )?),
                }
            }
            if a.get_is_group() && b.get_is_group() {
                require(
                    old.get_scope_id() == new.get_scope_id(),
                    "group parent changed",
                )?;
            }
            d.compare(a.get_is_group(), b.get_is_group())?;
        }
        (node::Enum(a), node::Enum(b)) => {
            d.compare(a.get_enumerants()?.len(), b.get_enumerants()?.len())?
        }
        (node::Interface(a), node::Interface(b)) => {
            let aa: alloc::collections::BTreeSet<_> =
                a.get_superclasses()?.iter().map(|s| s.get_id()).collect();
            let bb: alloc::collections::BTreeSet<_> =
                b.get_superclasses()?.iter().map(|s| s.get_id()).collect();
            if !aa.is_subset(&bb) {
                d.compare(1, 0)?;
            }
            if !bb.is_subset(&aa) {
                d.compare(0, 1)?;
            }
            let am = a.get_methods()?;
            let bm = b.get_methods()?;
            d.compare(am.len(), bm.len())?;
            for (a, b) in am.iter().zip(bm) {
                require(
                    a.get_param_struct_type() == b.get_param_struct_type()
                        && a.get_result_struct_type() == b.get_result_struct_type(),
                    "method parameter/result ID changed",
                )?;
            }
        }
        _ => (),
    }
    Ok(d.0 == Ordering::Greater)
}
fn types(
    a: type_::Reader<'_>,
    b: type_::Reader<'_>,
    d: &mut Direction,
    expectations: &mut Vec<Entry>,
    allow_struct: bool,
    depth: usize,
) -> Result<()> {
    require(depth < 64, "schema compatibility depth limit")?;
    let (aw, bw) = match (a.which(), b.which()) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(a), Err(b)) if a == b => return Ok(()),
        _ => return Err(invalid("unknown field type changed")),
    };
    let data_upgrade = |t: type_::Reader<'_>| -> Result<bool> {
        Ok(match t.which()? {
            type_::Text(()) => true,
            type_::List(l) => matches!(
                l.get_element_type()?.which()?,
                type_::Int8(()) | type_::Uint8(())
            ),
            _ => false,
        })
    };
    if matches!(bw, type_::Data(())) && data_upgrade(a)? {
        return d.compare(0, 1);
    }
    if matches!(aw, type_::Data(())) && data_upgrade(b)? {
        return d.compare(1, 0);
    }
    if matches!(bw, type_::AnyPointer(_))
        && validate::storage(a)?.1
        && !matches!(aw, type_::AnyPointer(_))
    {
        return d.compare(0, 1);
    }
    if matches!(aw, type_::AnyPointer(_))
        && validate::storage(b)?.1
        && !matches!(bw, type_::AnyPointer(_))
    {
        return d.compare(1, 0);
    }
    if allow_struct && core::mem::discriminant(&aw) != core::mem::discriminant(&bw) {
        if let type_::Struct(t) = aw {
            expectations.push(upgrade(b, t.get_type_id(), None)?);
            return Ok(());
        }
        if let type_::Struct(t) = bw {
            expectations.push(upgrade(a, t.get_type_id(), None)?);
            return Ok(());
        }
    }
    require(
        core::mem::discriminant(&aw) == core::mem::discriminant(&bw),
        "field type changed",
    )?;
    match (aw, bw) {
        (type_::Struct(a), type_::Struct(b)) => {
            require(a.get_type_id() == b.get_type_id(), "struct type ID changed")?
        }
        (type_::Enum(a), type_::Enum(b)) => {
            require(a.get_type_id() == b.get_type_id(), "enum type ID changed")?
        }
        (type_::Interface(a), type_::Interface(b)) => require(
            a.get_type_id() == b.get_type_id(),
            "interface type ID changed",
        )?,
        (type_::List(a), type_::List(b)) => types(
            a.get_element_type()?,
            b.get_element_type()?,
            d,
            expectations,
            true,
            depth + 1,
        )?,
        _ => (),
    }
    Ok(())
}
fn defaults(a: value::Reader<'_>, b: value::Reader<'_>) -> Result<()> {
    let equal = match (a.which()?, b.which()?) {
        (value::Bool(a), value::Bool(b)) => a == b,
        (value::Int8(a), value::Int8(b)) => a == b,
        (value::Int16(a), value::Int16(b)) => a == b,
        (value::Int32(a), value::Int32(b)) => a == b,
        (value::Int64(a), value::Int64(b)) => a == b,
        (value::Uint8(a), value::Uint8(b)) => a == b,
        (value::Uint16(a), value::Uint16(b)) => a == b,
        (value::Uint32(a), value::Uint32(b)) => a == b,
        (value::Uint64(a), value::Uint64(b)) => a == b,
        (value::Float32(a), value::Float32(b)) => a.to_bits() == b.to_bits(),
        (value::Float64(a), value::Float64(b)) => a.to_bits() == b.to_bits(),
        (value::Enum(a), value::Enum(b)) => a == b,
        _ => true, // Like C++, pointer defaults do not affect wire compatibility.
    };
    require(equal, "scalar default changed")
}

// Preserve the first slot's layout when a field becomes a group or a list
// element becomes a struct. Keep the constraint even if its target arrives later.
fn upgrade(
    t: type_::Reader<'_>,
    id: u64,
    position: Option<(node::Reader<'_>, field::Reader<'_>)>,
) -> Result<Entry> {
    let mut message = message::Builder::new_default();
    let mut n = message.init_root::<node::Builder>();
    n.set_id(id);
    n.set_display_name("(upgrade constraint)");
    let mut s = n.init_struct();
    let (bits, pointer) = validate::storage(t)?;
    s.set_data_word_count(u16::from(bits > 0));
    s.set_pointer_count(u16::from(pointer));
    if let Some((parent, _)) = position {
        let node::Struct(p) = parent.which()? else {
            unreachable!()
        };
        s.set_data_word_count(p.get_data_word_count());
        s.set_pointer_count(p.get_pointer_count());
    }
    let mut f = s.init_fields(1).get(0);
    f.set_name("member0");
    if let Some((_, field)) = position {
        match field.get_ordinal().which()? {
            field::ordinal::Implicit(()) => f.reborrow().init_ordinal().set_implicit(()),
            field::ordinal::Explicit(o) => f.reborrow().init_ordinal().set_explicit(o),
        }
    } else {
        f.reborrow().init_ordinal().set_explicit(0);
    }
    let mut slot = f.init_slot();
    slot.set_type(t)?;
    if let Some((_, field)) = position {
        let field::Slot(original) = field.which()? else {
            unreachable!()
        };
        slot.set_offset(original.get_offset());
        slot.set_default_value(original.get_default_value()?)?;
    } else {
        zero_default(t, slot.init_default_value())?;
    }
    Ok(Entry {
        message: Rc::new(message),
        stub: true,
    })
}
fn zero_default(t: type_::Reader<'_>, mut v: value::Builder<'_>) -> Result<()> {
    match t.which()? {
        type_::Void(()) => v.set_void(()),
        type_::Bool(()) => v.set_bool(false),
        type_::Int8(()) => v.set_int8(0),
        type_::Int16(()) => v.set_int16(0),
        type_::Int32(()) => v.set_int32(0),
        type_::Int64(()) => v.set_int64(0),
        type_::Uint8(()) => v.set_uint8(0),
        type_::Uint16(()) => v.set_uint16(0),
        type_::Uint32(()) => v.set_uint32(0),
        type_::Uint64(()) => v.set_uint64(0),
        type_::Float32(()) => v.set_float32(0.0),
        type_::Float64(()) => v.set_float64(0.0),
        type_::Text(()) => {
            v.init_text(0);
        }
        type_::Data(()) => {
            v.init_data(0);
        }
        type_::Enum(_) => v.set_enum(0),
        type_::Struct(_) => {
            v.init_struct();
        }
        type_::List(_) => {
            v.init_list();
        }
        type_::Interface(_) => v.set_interface(()),
        type_::AnyPointer(_) => {
            v.init_any_pointer();
        }
    }
    Ok(())
}
