// Copyright (c) 2013-2014 Sandstorm Development Group, Inc. and contributors
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

// Layout rules ported from capnp/compiler/node-translator.c++.

//! Ordinal-ordered struct layout, including overlapping union alternatives.
//!
//! This implements the pinned C++ compiler's allocation rules. Each alternative
//! tracks its own use of the union's shared data locations and pointer slots.
//! Ordinary groups reuse their parent's allocation scope; nested unions can
//! expand locations through enclosing alternatives when alignment permits it.
use crate::resolve::{Compilation, Resolved};
use crate::source::Graph;
use crate::syntax::{FieldKind, NodeKind};
use crate::Diagnostic;

type Result<T> = std::result::Result<T, &'static str>;
type Scope = Option<usize>; // None = the top-level struct; Some = union alternative.
const ISSUE_344: &str = "nested union layout is affected by Cap'n Proto issue #344; the pinned C++ compiler rejects this schema for wire compatibility";

#[derive(Clone, Copy, Default)]
struct Holes([u32; 6]);

impl Holes {
    fn allocate(&mut self, size: usize) -> Option<u32> {
        if size >= 6 {
            None
        } else if self.0[size] != 0 {
            Some(std::mem::take(&mut self.0[size]))
        } else {
            let result = self.allocate(size + 1)? * 2;
            self.0[size] = result + 1;
            Some(result)
        }
    }

    fn add(&mut self, mut size: usize, mut offset: u32, limit: usize) {
        while size < limit {
            debug_assert_eq!(self.0[size], 0);
            debug_assert_eq!(offset % 2, 1);
            self.0[size] = offset;
            size += 1;
            offset = offset.div_ceil(2);
        }
    }

    fn expand(&mut self, size: usize, offset: u32, factor: usize) -> bool {
        if factor == 0 {
            true
        } else if size >= 6 || self.0[size] != offset + 1 {
            false
        } else if self.expand(size + 1, offset >> 1, factor - 1) {
            self.0[size] = 0;
            true
        } else {
            false
        }
    }

    fn smallest(&self, size: usize) -> Option<usize> {
        (size..6).find(|&i| self.0[i] != 0)
    }
}

#[derive(Clone, Copy)]
struct Location {
    size: usize,
    offset: u32,
}

#[derive(Clone, Copy, Default)]
struct Usage {
    used: bool,
    size: usize,
    holes: Holes,
}

impl Usage {
    fn smallest(&self, location: Location, size: usize) -> Option<usize> {
        if !self.used {
            (size <= location.size).then_some(location.size)
        } else if size >= self.size {
            (size < location.size).then_some(size)
        } else {
            self.holes
                .smallest(size)
                .or_else(|| (self.size < location.size).then_some(self.size))
        }
    }

    fn allocate(&mut self, location: Location, size: usize) -> u32 {
        let offset;
        if !self.used {
            self.used = true;
            self.size = size;
            offset = 0;
        } else if size >= self.size {
            self.holes.add(self.size, 1, size);
            self.size = size + 1;
            offset = 1;
        } else if let Some(hole) = self.holes.allocate(size) {
            offset = hole;
        } else {
            offset = 1 << (self.size - size);
            self.holes.add(size, offset + 1, self.size);
            self.size += 1;
        }
        (location.offset << (location.size - size)) + offset
    }
}

struct Union {
    parent: Scope,
    members: usize,
    discriminant: Option<u32>,
    data: Vec<Location>,
    pointers: Vec<u32>,
}

struct Alternative {
    parent: usize,
    has_members: bool,
    data: Vec<Usage>,
    pointers: usize,
}

struct Layout {
    words: u32,
    pointers: u32,
    holes: Holes,
    unions: Vec<Union>,
    alternatives: Vec<Alternative>,
    budget: usize,
}

impl Layout {
    fn new(budget: usize) -> Self {
        Self {
            words: 0,
            pointers: 0,
            holes: Holes::default(),
            unions: vec![],
            alternatives: vec![],
            budget,
        }
    }

    fn step(&mut self) -> Result<()> {
        if self.budget == 0 {
            return Err("union layout work limit exceeded (1000000)");
        }
        self.budget -= 1;
        Ok(())
    }

    fn union(&mut self, parent: Scope) -> usize {
        let index = self.unions.len();
        self.unions.push(Union {
            parent,
            members: 0,
            discriminant: None,
            data: vec![],
            pointers: vec![],
        });
        index
    }

    fn alternative(&mut self, parent: usize) -> usize {
        let index = self.alternatives.len();
        self.alternatives.push(Alternative {
            parent,
            has_members: false,
            data: vec![],
            pointers: 0,
        });
        index
    }

    fn discriminant(&mut self, union: usize) -> Result<bool> {
        if self.unions[union].discriminant.is_some() {
            return Ok(false);
        }
        let offset = self.data(self.unions[union].parent, 4)?;
        self.unions[union].discriminant = Some(offset);
        Ok(true)
    }

    fn member(&mut self, alternative: usize) -> Result<()> {
        if !self.alternatives[alternative].has_members {
            self.alternatives[alternative].has_members = true;
            let union = self.alternatives[alternative].parent;
            self.unions[union].members += 1;
            if self.unions[union].members == 2 {
                self.discriminant(union)?;
            }
        }
        Ok(())
    }

    fn void(&mut self, scope: Scope) -> Result<u32> {
        self.step()?;
        if let Some(alternative) = scope {
            self.member(alternative)?;
            let union = self.alternatives[alternative].parent;
            // Void must still notify enclosing unions about their alternatives.
            self.void(self.unions[union].parent)?;
        }
        Ok(0)
    }

    fn pointer(&mut self, scope: Scope) -> Result<u32> {
        self.step()?;
        let Some(alternative) = scope else {
            let result = self.pointers;
            self.pointers += 1;
            return Ok(result);
        };
        self.member(alternative)?;
        let union = self.alternatives[alternative].parent;
        let used = self.alternatives[alternative].pointers;
        self.alternatives[alternative].pointers += 1;
        if let Some(&offset) = self.unions[union].pointers.get(used) {
            return Ok(offset);
        }
        let offset = self.pointer(self.unions[union].parent)?;
        self.unions[union].pointers.push(offset);
        Ok(offset)
    }

    fn expand_location(&mut self, union: usize, index: usize, size: usize) -> Result<bool> {
        let location = self.unions[union].data[index];
        if size <= location.size {
            return Ok(true);
        }
        if !self.expand(
            self.unions[union].parent,
            location.size,
            location.offset,
            size - location.size,
        )? {
            return Ok(false);
        }
        self.unions[union].data[index] = Location {
            size,
            offset: location.offset >> (size - location.size),
        };
        Ok(true)
    }

    fn expand_usage(
        &mut self,
        union: usize,
        index: usize,
        usage: &mut Usage,
        size: usize,
        new_holes: bool,
    ) -> Result<bool> {
        if !self.expand_location(union, index, size)? {
            return Ok(false);
        }
        if new_holes {
            usage.holes.add(usage.size, 1, size);
        } else {
            // Preserve the reference compiler's rejection of schemas whose
            // layout changed when the historical nested-union bug was fixed.
            return Err(ISSUE_344);
        }
        usage.size = size;
        Ok(true)
    }

    fn expand(&mut self, scope: Scope, size: usize, offset: u32, factor: usize) -> Result<bool> {
        self.step()?;
        let Some(alternative) = scope else {
            return Ok(self.holes.expand(size, offset, factor));
        };
        let must_fail = size + factor > 6 || (offset & ((1 << factor) - 1)) != 0;
        let union = self.alternatives[alternative].parent;
        for index in 0..self.alternatives[alternative].data.len() {
            self.step()?;
            let location = self.unions[union].data[index];
            if location.size >= size && offset >> (location.size - size) == location.offset {
                let local = offset - (location.offset << (location.size - size));
                let mut usage = self.alternatives[alternative].data[index];
                let result = if local == 0 && usage.size == size {
                    self.expand_usage(union, index, &mut usage, size + factor, false)?
                } else {
                    usage.holes.expand(size, local, factor)
                };
                if must_fail && result {
                    return Err(ISSUE_344);
                }
                self.alternatives[alternative].data[index] = usage;
                return Ok(result);
            }
        }
        Err("cannot expand an unallocated union location")
    }

    fn data(&mut self, scope: Scope, size: usize) -> Result<u32> {
        self.step()?;
        let Some(alternative) = scope else {
            if let Some(hole) = self.holes.allocate(size) {
                return Ok(hole);
            }
            let offset = self.words << (6 - size);
            self.words += 1;
            self.holes.add(size, offset + 1, 6);
            return Ok(offset);
        };
        self.member(alternative)?;
        let union = self.alternatives[alternative].parent;
        self.alternatives[alternative]
            .data
            .resize(self.unions[union].data.len(), Usage::default());
        let mut best = None;
        for index in 0..self.unions[union].data.len() {
            self.step()?;
            let usage = self.alternatives[alternative].data[index];
            if let Some(hole) = usage.smallest(self.unions[union].data[index], size) {
                if best.is_none_or(|(best_size, _)| hole < best_size) {
                    best = Some((hole, index));
                }
            }
        }
        if let Some((_, index)) = best {
            return Ok(self.alternatives[alternative].data[index]
                .allocate(self.unions[union].data[index], size));
        }
        for index in 0..self.unions[union].data.len() {
            self.step()?;
            let mut usage = self.alternatives[alternative].data[index];
            let local;
            if !usage.used {
                if !self.expand_location(union, index, size)? {
                    continue;
                }
                usage.used = true;
                usage.size = size;
                local = 0;
            } else {
                let desired = usage.size.max(size) + 1;
                if !self.expand_usage(union, index, &mut usage, desired, true)? {
                    continue;
                }
                local = usage.holes.allocate(size).expect("expansion added a hole");
            }
            let location = self.unions[union].data[index];
            self.alternatives[alternative].data[index] = usage;
            return Ok((location.offset << (location.size - size)) + local);
        }
        let offset = self.data(self.unions[union].parent, size)?;
        self.unions[union].data.push(Location { size, offset });
        self.alternatives[alternative].data.push(Usage {
            used: true,
            size,
            holes: Holes::default(),
        });
        Ok(offset)
    }

    fn field(&mut self, scope: Scope, ty: &Resolved) -> Result<u32> {
        match ty {
            Resolved::Void => self.void(scope),
            Resolved::Bool => self.data(scope, 0),
            Resolved::Int8 | Resolved::UInt8 => self.data(scope, 3),
            Resolved::Int16 | Resolved::UInt16 | Resolved::Enum(..) => self.data(scope, 4),
            Resolved::Int32 | Resolved::UInt32 | Resolved::Float32 => self.data(scope, 5),
            Resolved::Int64 | Resolved::UInt64 | Resolved::Float64 => self.data(scope, 6),
            _ => self.pointer(scope),
        }
    }
}

#[derive(Clone, Default)]
pub(crate) struct StructLayout {
    pub words: u16,
    pub pointers: u16,
    pub discriminant: u32,
    pub offsets: Vec<u32>,
}

struct Event {
    ordinal: u16,
    node: usize,
    field: usize,
    scope: Scope,
    union: Option<usize>, // Explicit ordinal for a legacy named union.
}

fn scopes(
    graph: &Graph,
    node: usize,
    scope: Scope,
    layout: &mut Layout,
    unions: &mut Vec<(usize, usize)>,
    events: &mut Vec<Event>,
) -> Option<usize> {
    let NodeKind::Struct(fields) = &graph.nodes[node].kind else {
        unreachable!()
    };
    let union = fields.iter().any(|f| f.in_union).then(|| {
        let union = layout.union(scope);
        unions.push((node, union));
        union
    });
    for (index, field) in fields.iter().enumerate() {
        let field_scope = if field.in_union {
            Some(layout.alternative(union.unwrap()))
        } else {
            scope
        };
        let child_union = if let FieldKind::Group(group) = field.kind {
            scopes(graph, group, field_scope, layout, unions, events)
        } else {
            None
        };
        if let Some(ordinal) = field.ordinal {
            events.push(Event {
                ordinal,
                node,
                field: index,
                scope: field_scope,
                union: child_union,
            });
        }
    }
    union
}

pub(crate) fn compile(
    graph: &Graph,
    compiled: &Compilation,
) -> std::result::Result<Vec<StructLayout>, Diagnostic> {
    let mut result = vec![StructLayout::default(); graph.nodes.len()];
    let mut budget = 1_000_000;
    for &root in &compiled.required {
        if graph.nodes[root].is_group || !matches!(graph.nodes[root].kind, NodeKind::Struct(_)) {
            continue;
        }
        let mut layout = Layout::new(budget);
        let mut unions = Vec::new();
        let mut events = Vec::new();
        scopes(graph, root, None, &mut layout, &mut unions, &mut events);
        events.sort_by_key(|e| e.ordinal);
        let mut members = std::collections::BTreeSet::from([root]);
        for event in events {
            let NodeKind::Struct(fields) = &graph.nodes[event.node].kind else {
                unreachable!()
            };
            let error = |message| {
                graph
                    .source(event.node)
                    .error(fields[event.field].span, message)
            };
            let offset = if let Some(union) = event.union {
                if !layout.discriminant(union).map_err(error)? {
                    return Err(error(
                        "a legacy union ordinal may follow at most one union member ordinal",
                    ));
                }
                0
            } else {
                let ty = compiled.fields[event.node].as_ref().unwrap()[event.field]
                    .as_ref()
                    .unwrap();
                layout.field(event.scope, &ty.ty).map_err(error)?
            };
            result[event.node].offsets.resize(fields.len(), 0);
            result[event.node].offsets[event.field] = offset;
            let mut ancestor = event.node;
            while ancestor != root {
                members.insert(ancestor);
                ancestor = graph.nodes[ancestor].parent.unwrap();
            }
        }
        for (node, union) in unions {
            layout
                .discriminant(union)
                .map_err(|m| graph.source(node).error(graph.nodes[node].span, m))?;
            result[node].discriminant = layout.unions[union].discriminant.unwrap();
        }
        let error = || {
            graph.source(root).error(
                graph.nodes[root].span,
                "struct layout exceeds 65535 words or pointers",
            )
        };
        let words = u16::try_from(layout.words).map_err(|_| error())?;
        let pointers = u16::try_from(layout.pointers).map_err(|_| error())?;
        for node in members {
            result[node].words = words;
            result[node].pointers = pointers;
        }
        budget = layout.budget;
    }
    Ok(result)
}
