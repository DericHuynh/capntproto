//! Encode composite values through checked, schema-free Cap'n Proto builders.
//! Unassigned fields stay zero on the wire; scalar assignments XOR their schema
//! defaults. Assignment order and group initialization follow the reference.
use crate::layout::StructLayout;
use crate::resolve::{Compilation, Resolved};
use crate::source::Graph;
use crate::syntax::{FieldKind, NodeKind};
use crate::value::{StructValue, TypedValue, Value};
use capnp::{any_pointer, any_struct, primitive_list, schema_capnp::value, Result};

fn error(message: &str) -> capnp::Error {
    capnp::Error::failed(message.into())
}

pub(crate) struct Encoder<'a> {
    graph: &'a Graph,
    compiled: &'a Compilation,
    layouts: &'a [StructLayout],
    bytes: usize,
    steps: usize,
}

impl<'a> Encoder<'a> {
    pub fn new(graph: &'a Graph, compiled: &'a Compilation, layouts: &'a [StructLayout]) -> Self {
        Self {
            graph,
            compiled,
            layouts,
            bytes: 16 * 1024 * 1024,
            steps: 1_000_000,
        }
    }

    fn charge(&mut self, steps: usize, bytes: usize) -> Result<()> {
        self.steps = self
            .steps
            .checked_sub(steps)
            .ok_or_else(|| error("composite encoding work limit exceeded (1000000)"))?;
        self.bytes = self
            .bytes
            .checked_sub(bytes)
            .ok_or_else(|| error("encoded value size limit exceeded (16 MiB)"))?;
        Ok(())
    }

    pub fn emit(&mut self, value: &TypedValue, builder: value::Builder<'_>) -> Result<()> {
        if matches!(value.value, Value::Struct(_) | Value::List(_)) {
            let pointer = match value.ty {
                Resolved::Struct(..) => builder.init_struct(),
                Resolved::List(_) => builder.init_list(),
                Resolved::AnyPointer | Resolved::AnyStruct | Resolved::AnyList => {
                    builder.init_any_pointer()
                }
                _ => unreachable!("composite value has a pointer type"),
            };
            self.pointer(&value.value, pointer)
        } else {
            self.charge(1, value.value.pointer_bytes().div_ceil(8) * 8)?;
            value.value.emit(&value.ty, builder);
            Ok(())
        }
    }

    fn pointer(&mut self, value: &Value, mut pointer: any_pointer::Builder<'_>) -> Result<()> {
        self.charge(1, 0)?;
        match value {
            Value::Text(Some(text)) => {
                self.charge(0, (text.len() + 1).div_ceil(8) * 8)?;
                pointer.set_as::<capnp::text::Owned>(capnp::text::Reader(text))?;
            }
            Value::Data(Some(data)) => {
                self.charge(0, data.len().div_ceil(8) * 8)?;
                pointer.set_as::<capnp::data::Owned>(data.as_ref())?;
            }
            Value::Text(None) | Value::Data(None) | Value::Null => pointer.clear(),
            Value::Struct(value) if value.embedded.is_some() => {
                let embedded = value.embedded.as_ref().unwrap();
                self.charge(embedded.size / 8 + 1, embedded.size)?;
                let reader = embedded.reader()?;
                pointer
                    .set_as::<any_pointer::Owned>(reader.get_root::<any_struct::Reader<'_>>()?)?;
            }
            Value::Struct(value) => {
                let layout = &self.layouts[value.node];
                self.charge(
                    0,
                    (usize::from(layout.words) + usize::from(layout.pointers)) * 8,
                )?;
                let mut builder = pointer.init_as_any_struct(layout.words, layout.pointers);
                self.structure(value, &mut builder)?;
            }
            Value::List(list) => {
                let count = u32::try_from(list.values.len())
                    .map_err(|_| error("list element count exceeds UInt32"))?;
                self.charge(list.values.len(), 0)?;
                if let Some(bits) = list.element.data_bits() {
                    self.charge(0, (list.values.len() * bits).div_ceil(64) * 8)?;
                    macro_rules! primitive {
                        ($type:ty) => {{
                            let mut out =
                                pointer.initn_as::<primitive_list::Builder<'_, $type>>(count);
                            for (i, v) in list.values.iter().enumerate() {
                                out.set(i as u32, v.bits(&list.element) as $type);
                            }
                        }};
                    }
                    match bits {
                        0 => {
                            pointer.initn_as::<primitive_list::Builder<'_, ()>>(count);
                        }
                        1 => {
                            let mut out =
                                pointer.initn_as::<primitive_list::Builder<'_, bool>>(count);
                            for (i, v) in list.values.iter().enumerate() {
                                out.set(i as u32, v.bits(&list.element) != 0);
                            }
                        }
                        8 => primitive!(u8),
                        16 => primitive!(u16),
                        32 => primitive!(u32),
                        64 => primitive!(u64),
                        _ => unreachable!("supported scalar width"),
                    }
                } else if let Resolved::Struct(node, _) = list.element {
                    let layout = &self.layouts[node];
                    let words = usize::from(layout.words) + usize::from(layout.pointers);
                    let bytes = list
                        .values
                        .len()
                        .checked_mul(words)
                        .and_then(|n| n.checked_add(1))
                        .and_then(|n| n.checked_mul(8))
                        .ok_or_else(|| error("struct list size overflow"))?;
                    self.charge(0, bytes)?;
                    let mut out =
                        pointer.init_as_list_of_any_struct(layout.words, layout.pointers, count)?;
                    for (i, value) in list.values.iter().enumerate() {
                        let Value::Struct(value) = value else {
                            unreachable!("struct list was type-checked")
                        };
                        self.structure(value, &mut out.reborrow().get(i as u32))?;
                    }
                } else {
                    self.charge(0, list.values.len() * 8)?;
                    let mut out = pointer.initn_as::<capnp::any_pointer_list::Builder<'_>>(count);
                    for (i, value) in list.values.iter().enumerate() {
                        self.pointer(value, out.reborrow().get(i as u32))?;
                    }
                }
            }
            _ => return Err(error("scalar value used as a pointer")),
        }
        Ok(())
    }

    fn structure(
        &mut self,
        value: &StructValue,
        builder: &mut any_struct::Builder<'_>,
    ) -> Result<()> {
        if let Some(embedded) = &value.embedded {
            self.charge(embedded.size / 8 + 1, embedded.size)?;
            let reader = embedded.reader()?;
            let value = reader.get_root::<any_struct::Reader<'_>>()?;
            // Inline struct lists have fixed element storage. C++ copies the
            // intersection, retaining zeroes for fields absent in older data.
            let data = value.get_data_section();
            let target = builder.get_data_section();
            let count = data.len().min(target.len());
            target[..count].copy_from_slice(&data[..count]);
            let pointers = value.get_pointer_section();
            let mut target = builder.get_pointer_section();
            for i in 0..pointers.len().min(target.len()) {
                target
                    .reborrow()
                    .get(i)
                    .set_as::<any_pointer::Owned>(pointers.get(i))?;
            }
            return Ok(());
        }
        let NodeKind::Struct(fields) = &self.graph.nodes[value.node].kind else {
            unreachable!()
        };
        let node = value.node;
        for (index, value) in &value.fields {
            self.charge(1, 0)?;
            self.activate(node, *index, builder)?;
            match fields[*index].kind {
                FieldKind::Group(group) => {
                    self.clear_group(group, builder)?;
                    let Value::Struct(value) = value else {
                        unreachable!("group was type-checked")
                    };
                    self.structure(value, builder)?;
                }
                FieldKind::Slot { .. } => {
                    let default = self.compiled.fields[node].as_ref().unwrap()[*index]
                        .as_ref()
                        .unwrap();
                    let offset = self.layouts[node].offsets[*index] as usize;
                    if let Some(bits) = default.ty.data_bits() {
                        Self::write_bits(
                            builder,
                            offset,
                            bits,
                            value.bits(&default.ty) ^ default.value.bits(&default.ty),
                        )?;
                    } else {
                        self.pointer(value, builder.get_pointer_section().get(offset as u32))?;
                    }
                }
            }
        }
        Ok(())
    }

    fn activate(
        &self,
        node: usize,
        index: usize,
        builder: &mut any_struct::Builder<'_>,
    ) -> Result<()> {
        let NodeKind::Struct(fields) = &self.graph.nodes[node].kind else {
            unreachable!()
        };
        if fields[index].in_union {
            let tag = fields[..index].iter().filter(|f| f.in_union).count() as u64;
            Self::write_bits(builder, self.layouts[node].discriminant as usize, 16, tag)?;
        }
        Ok(())
    }

    fn clear_group(&mut self, node: usize, builder: &mut any_struct::Builder<'_>) -> Result<()> {
        let NodeKind::Struct(fields) = &self.graph.nodes[node].kind else {
            unreachable!()
        };
        // C++ clears the default union alternative, then non-union fields.
        // Other alternatives can leave inactive bits/pointers in shared storage.
        let first = fields.iter().position(|f| f.in_union);
        for index in first.into_iter().chain(
            fields
                .iter()
                .enumerate()
                .filter(|(_, f)| !f.in_union)
                .map(|(i, _)| i),
        ) {
            self.charge(1, 0)?;
            self.activate(node, index, builder)?;
            match fields[index].kind {
                FieldKind::Group(group) => self.clear_group(group, builder)?,
                FieldKind::Slot { .. } => {
                    let value = self.compiled.fields[node].as_ref().unwrap()[index]
                        .as_ref()
                        .unwrap();
                    let offset = self.layouts[node].offsets[index] as usize;
                    if let Some(bits) = value.ty.data_bits() {
                        Self::write_bits(builder, offset, bits, 0)?;
                    } else {
                        builder.get_pointer_section().get(offset as u32).clear();
                    }
                }
            }
        }
        Ok(())
    }

    fn write_bits(
        builder: &mut any_struct::Builder<'_>,
        offset: usize,
        bits: usize,
        value: u64,
    ) -> Result<()> {
        if bits == 0 {
            return Ok(());
        }
        let data = builder.get_data_section();
        if bits == 1 {
            let byte = data
                .get_mut(offset / 8)
                .ok_or_else(|| error("field offset exceeds struct layout"))?;
            let mask = 1 << (offset % 8);
            *byte = (*byte & !mask) | if value & 1 != 0 { mask } else { 0 };
        } else {
            let width = bits / 8;
            let start = offset * width;
            let bytes = data
                .get_mut(start..start + width)
                .ok_or_else(|| error("field offset exceeds struct layout"))?;
            bytes.copy_from_slice(&value.to_le_bytes()[..width]);
        }
        Ok(())
    }
}
