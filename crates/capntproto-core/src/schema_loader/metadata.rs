//! Constant and annotation values retain the owning loader's schema identity.
use super::*;
use crate::schema_capnp::{annotation, enumerant};

impl<'a> Schema<'a> {
    pub fn constant_type(&self) -> Result<Type<'a>> {
        let node::Const(c) = self.get_proto().which()? else {
            return Err(invalid("not a constant schema"));
        };
        self.get_type(c.get_type()?)
    }

    /// Borrow a constant's value from this loader. Aggregate values keep their
    /// resolved schema and brand, just like values read from a dynamic message.
    /// Unknown types remain inspectable; unresolved parameters and mismatched
    /// value tags return an error. Pointer contents are checked when accessed.
    ///
    /// Values cannot escape the owning loader:
    /// ```compile_fail
    /// use capnp::schema_loader::SchemaLoader;
    /// let value = {
    ///     let loader = SchemaLoader::default();
    ///     loader.get(1).unwrap().constant_value().unwrap()
    /// };
    /// drop(value);
    /// ```
    /// A live value also prevents schema replacement:
    /// ```compile_fail
    /// use capnp::{message, schema_capnp::node, schema_loader::SchemaLoader};
    /// let mut loader = SchemaLoader::default();
    /// let value = loader.get(1).unwrap().constant_value().unwrap();
    /// let mut node = message::Builder::new_default();
    /// node.init_root::<node::Builder>().set_id(2);
    /// loader.load(node.get_root_as_reader().unwrap()).unwrap();
    /// drop(value);
    /// ```
    pub fn constant_value(&self) -> Result<dynamic::Value<'a, 'a>> {
        let node::Const(c) = self.get_proto().which()? else {
            return Err(invalid("not a constant schema"));
        };
        dynamic::read_schema_value(c.get_value()?, self.constant_type()?)
    }

    pub fn annotations(&self) -> Result<AnnotationList<'a>> {
        Ok(AnnotationList {
            scope: self.clone(),
            raw: self.get_proto().get_annotations()?,
        })
    }

    pub fn enumerants(&self) -> Result<Vec<Enumerant<'a>>> {
        let node::Enum(e) = self.get_proto().which()? else {
            return Err(invalid("not an enum schema"));
        };
        Ok((0..e.get_enumerants()?.len())
            .map(|index| Enumerant {
                parent: self.clone(),
                index: u16::try_from(index).expect("validated member count"),
            })
            .collect())
    }

    pub(super) fn enumerant_at(&self, index: u16) -> Result<Option<Enumerant<'a>>> {
        let node::Enum(e) = self.get_proto().which()? else {
            return Err(invalid("not an enum schema"));
        };
        Ok(
            (u32::from(index) < e.get_enumerants()?.len()).then(|| Enumerant {
                parent: self.clone(),
                index,
            }),
        )
    }

    pub fn enumerant(&self, name: &str) -> Result<Enumerant<'a>> {
        self.find_enumerant(name)?
            .ok_or_else(|| invalid("no such enumerant"))
    }

    pub fn find_enumerant(&self, name: &str) -> Result<Option<Enumerant<'a>>> {
        for member in self.enumerants()? {
            if member.get_proto().get_name()?.as_bytes() == name.as_bytes() {
                return Ok(Some(member));
            }
        }
        Ok(None)
    }
}

impl<'a> Field<'a> {
    pub fn annotations(&self) -> Result<AnnotationList<'a>> {
        Ok(AnnotationList {
            scope: self.parent.clone(),
            raw: self.get_proto().get_annotations()?,
        })
    }
}

impl<'a> Method<'a> {
    pub fn annotations(&self) -> Result<AnnotationList<'a>> {
        Ok(AnnotationList {
            scope: self.scope()?,
            raw: self.get_proto().get_annotations()?,
        })
    }
}

#[derive(Clone)]
pub struct Enumerant<'a> {
    parent: Schema<'a>,
    index: u16,
}
impl<'a> Enumerant<'a> {
    pub fn identity(&self) -> Result<crate::schema::MemberIdentity<'a>> {
        Ok(self
            .parent
            .identity()?
            .member(crate::schema::MemberKind::Enumerant, self.index))
    }
    pub fn parent(&self) -> Schema<'a> {
        self.parent.clone()
    }
    pub fn index(&self) -> u16 {
        self.index
    }
    pub fn get_proto(&self) -> enumerant::Reader<'a> {
        let node::Enum(e) = self.parent.get_proto().which().unwrap() else {
            unreachable!()
        };
        e.get_enumerants().unwrap().get(u32::from(self.index))
    }
    pub fn annotations(&self) -> Result<AnnotationList<'a>> {
        Ok(AnnotationList {
            scope: self.parent.clone(),
            raw: self.get_proto().get_annotations()?,
        })
    }
}

/// An applied annotation, resolved against the same loader as its containing
/// node. Resolution is lazy: a missing declaration is an error on access and
/// does not prevent inspecting unrelated annotations or the original schema.
#[derive(Clone)]
pub struct Annotation<'a> {
    declaration: Schema<'a>,
    raw: annotation::Reader<'a>,
}
impl<'a> Annotation<'a> {
    pub fn id(&self) -> u64 {
        self.raw.get_id()
    }
    pub fn get_proto(&self) -> annotation::Reader<'a> {
        self.raw
    }
    pub fn declaration(&self) -> Schema<'a> {
        self.declaration.clone()
    }
    pub fn get_type(&self) -> Result<Type<'a>> {
        let node::Annotation(a) = self.declaration.get_proto().which()? else {
            return Err(invalid("not an annotation schema"));
        };
        self.declaration.get_type(a.get_type()?)
    }
    /// Decode using the applied brand. Unresolved generic parameters and values
    /// whose union tag disagrees with the resolved type return an error.
    pub fn get_value(&self) -> Result<dynamic::Value<'a, 'a>> {
        dynamic::read_schema_value(self.raw.get_value()?, self.get_type()?)
    }
}

#[derive(Clone)]
pub struct AnnotationList<'a> {
    scope: Schema<'a>,
    raw: crate::struct_list::Reader<'a, annotation::Owned>,
}
impl<'a> AnnotationList<'a> {
    pub fn len(&self) -> u32 {
        self.raw.len()
    }
    pub fn is_empty(&self) -> bool {
        self.raw.is_empty()
    }
    pub fn get(&self, index: u32) -> Result<Annotation<'a>> {
        require(index < self.len(), "annotation index out of bounds")?;
        let raw = self.raw.get(index);
        let declaration = self.scope.loader.get(raw.get_id())?;
        require(
            declaration.kind() == Kind::Annotation,
            "not an annotation schema",
        )?;
        Ok(Annotation {
            declaration: declaration.bind(raw.get_brand()?, Some(&self.scope))?,
            raw,
        })
    }
    pub fn find(&self, id: u64) -> Result<Option<Annotation<'a>>> {
        for index in 0..self.len() {
            if self.raw.get(index).get_id() == id {
                return self.get(index).map(Some);
            }
        }
        Ok(None)
    }
    pub fn iter(&self) -> impl ExactSizeIterator<Item = Result<Annotation<'a>>> + '_ {
        (0..self.len()).map(|index| self.get(index))
    }
}
