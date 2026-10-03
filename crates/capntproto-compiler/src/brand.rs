//! Generic environments. Bindings share their trees; expanding or emitting them
//! is charged to the frontend's existing resolution budget.
use crate::{resolve::Resolved, syntax::Node};
use capnp::schema_capnp::brand;
use std::{collections::BTreeMap, sync::Arc};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Scope {
    Inherit,
    Bind(Arc<[Resolved]>),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Brand(pub Vec<(usize, Scope)>);

impl Brand {
    pub fn get(&self, index: usize) -> Option<&Scope> {
        self.0.iter().find(|(i, _)| *i == index).map(|(_, s)| s)
    }
    pub fn bind(&mut self, index: usize, types: Vec<Resolved>) {
        self.0.retain(|(i, _)| *i != index);
        self.0.insert(0, (index, Scope::Bind(types.into())));
    }
    pub fn lexical(nodes: &[Node], mut index: usize) -> Self {
        let mut result = Self::default();
        loop {
            if !nodes[index].parameters.is_empty() {
                result.0.push((index, Scope::Inherit));
            }
            let Some(parent) = nodes[index].parent else {
                break;
            };
            index = parent;
        }
        result
    }

    pub fn for_node(nodes: &[Node], index: usize, context: &Self) -> Self {
        let mut result = Self::default();
        let mut parent = nodes[index].parent;
        while let Some(index) = parent {
            if let Some(scope) = context.get(index) {
                result.0.push((index, scope.clone()));
            }
            parent = nodes[index].parent;
        }
        result
    }

    pub fn emit(&self, builder: brand::Builder<'_>, nodes: &[Node]) {
        if self.0.is_empty() {
            return;
        }
        // Innermost first. Aliases can retain scopes outside the target's
        // lexical ancestry, so declaration indices cannot determine this order.
        let mut scopes = builder.init_scopes(self.0.len() as u32);
        for (i, (index, scope)) in self.0.iter().enumerate() {
            let mut out = scopes.reborrow().get(i as u32);
            out.set_scope_id(nodes[*index].id);
            match scope {
                Scope::Inherit => out.set_inherit(()),
                Scope::Bind(types) => {
                    let mut bindings = out.init_bind(types.len() as u32);
                    for (i, ty) in types.iter().enumerate() {
                        ty.emit(bindings.reborrow().get(i as u32).init_type(), nodes);
                    }
                }
            }
        }
    }

    pub fn substitute(
        &self,
        context: &Self,
        depth: usize,
        budget: &mut usize,
    ) -> Result<Self, &'static str> {
        charge(depth, budget)?;
        let mut result = Self::default();
        for (index, scope) in &self.0 {
            let index = *index;
            match scope {
                Scope::Inherit => {
                    if let Some(scope) = context.get(index) {
                        result.0.push((index, scope.clone()));
                    }
                }
                Scope::Bind(types) => {
                    let types = types
                        .iter()
                        .map(|t| t.substitute(context, depth + 1, budget))
                        .collect::<Result<Vec<_>, _>>()?;
                    result.0.push((index, Scope::Bind(types.into())));
                }
            }
        }
        Ok(result)
    }
}

pub fn charge(depth: usize, budget: &mut usize) -> Result<(), &'static str> {
    if depth >= 64 {
        return Err("expanded generic type nesting limit exceeded (64)");
    }
    *budget = budget
        .checked_sub(1)
        .ok_or("type resolution work limit exceeded (1000000)")?;
    Ok(())
}

impl Resolved {
    pub fn declaration_mut(&mut self) -> Option<(usize, &mut Brand)> {
        match self {
            Self::Struct(index, brand)
            | Self::Enum(index, brand)
            | Self::Interface(index, brand)
            | Self::Constant(index, brand)
            | Self::Annotation(index, brand) => Some((*index, brand)),
            _ => None,
        }
    }

    pub fn substitute(
        &self,
        context: &Brand,
        depth: usize,
        budget: &mut usize,
    ) -> Result<Self, &'static str> {
        charge(depth, budget)?;
        Ok(match self {
            Self::Parameter(scope, index) => match context.get(*scope) {
                Some(Scope::Inherit) => self.clone(),
                Some(Scope::Bind(types)) => types
                    .get(*index as usize)
                    .cloned()
                    .unwrap_or(Self::AnyPointer),
                None => Self::AnyPointer,
            },
            Self::List(element) => {
                Self::List(Box::new(element.substitute(context, depth + 1, budget)?))
            }
            _ => {
                let mut result = self.clone();
                if let Some((_, brand)) = result.declaration_mut() {
                    *brand = brand.substitute(context, depth + 1, budget)?;
                }
                result
            }
        })
    }

    pub fn check(&self, depth: usize, budget: &mut usize) -> Result<(), &'static str> {
        charge(depth, budget)?;
        match self {
            Self::List(element) => element.check(depth + 1, budget)?,
            Self::Struct(_, brand)
            | Self::Enum(_, brand)
            | Self::Interface(_, brand)
            | Self::Constant(_, brand)
            | Self::Annotation(_, brand) => {
                for (_, scope) in &brand.0 {
                    charge(depth, budget)?;
                    if let Scope::Bind(types) = scope {
                        for ty in types.iter() {
                            ty.check(depth + 1, budget)?;
                        }
                    }
                }
            }
            _ => (),
        }
        Ok(())
    }
}

impl Brand {
    pub fn equivalent(&self, other: &Self) -> bool {
        let normalized = |brand: &Brand| {
            brand
                .0
                .iter()
                .map(|(i, s)| (*i, s.clone()))
                .collect::<BTreeMap<_, _>>()
        };
        normalized(self) == normalized(other)
    }
}
