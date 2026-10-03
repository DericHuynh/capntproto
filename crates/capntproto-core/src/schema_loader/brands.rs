//! Applied generic arguments, including explicit defaults and inherited scopes.
use super::*;

/// Arguments actually supplied for one scope, not the parameter declaration's
/// arity. Missing arguments read as AnyPointer; an unbound scope instead yields
/// symbolic parameters, including beyond the currently declared arity.
#[derive(Clone, PartialEq)]
pub struct BrandArguments<'a> {
    scope_id: u64,
    values: Rc<Vec<Type<'a>>>,
    unbound: bool,
}
impl<'a> BrandArguments<'a> {
    pub(super) fn write_identity(&self, key: &mut crate::schema::identity::Builder) -> Result<()> {
        use crate::schema::identity::Part;
        key.spend()?;
        key.parts.push(Part::Scope {
            id: self.scope_id,
            unbound: self.unbound,
        });
        key.parts.push(Part::Length(self.values.len()));
        for ty in self.values.iter() {
            ty.write_identity(key)?;
        }
        Ok(())
    }
    pub(super) fn bound(scope_id: u64, values: Vec<Type<'a>>) -> Self {
        Self {
            scope_id,
            values: Rc::new(values),
            unbound: false,
        }
    }
    pub fn len(&self) -> u32 {
        // Arguments are bounded by the validated schema parameter list.
        u32::try_from(self.values.len()).expect("brand argument count fits schema list bounds")
    }
    pub fn is_empty(&self) -> bool {
        self.values.is_empty()
    }
    pub fn get(&self, index: u16) -> Type<'a> {
        self.values.get(usize::from(index)).cloned().unwrap_or({
            if self.unbound {
                Type::Parameter(self.scope_id, index)
            } else {
                Type::AnyPointer(PointerKind::Any)
            }
        })
    }
    /// Iterate only explicitly supplied arguments. Symbolic unbound scopes
    /// have length zero, but `get()` can still inspect any parameter index.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = Type<'a>> + '_ {
        self.values.iter().cloned()
    }
}

impl<'a> Schema<'a> {
    pub fn is_branded(&self) -> bool {
        self.unbound || !self.bindings.is_empty()
    }
    /// Remove all arguments. The default brand substitutes AnyPointer;
    /// `SchemaLoader::get_unbound()` supplies symbolic parameters instead.
    pub fn generic(&self) -> Self {
        Self {
            loader: self.loader,
            id: self.id,
            bindings: Rc::new(BTreeMap::new()),
            unbound: false,
        }
    }
    /// Explicit scope IDs in the applied brand, sorted by ID. Default and
    /// wholly unbound brands have no explicit scopes. This does not walk the
    /// lexical parent chain or require those parent nodes to be loaded.
    pub fn generic_scope_ids(&self) -> Vec<u64> {
        if self.get_proto().get_is_generic() {
            self.bindings.keys().copied().collect()
        } else {
            Vec::new()
        }
    }
    pub fn brand_arguments_at_scope(&self, scope_id: u64) -> Result<BrandArguments<'a>> {
        require(self.get_proto().get_is_generic(), "not a generic schema")?;
        Ok(self.arguments_at_scope(scope_id))
    }
    pub(super) fn arguments_at_scope(&self, scope_id: u64) -> BrandArguments<'a> {
        self.bindings
            .get(&scope_id)
            .cloned()
            .unwrap_or_else(|| BrandArguments {
                scope_id,
                values: Rc::new(Vec::new()),
                unbound: self.unbound,
            })
    }
}
