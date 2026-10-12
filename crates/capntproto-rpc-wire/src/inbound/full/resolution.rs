use super::*;
use capntproto_rpc_engine::ids::ImportId;

#[must_use]
pub struct Resolution<'b, 'r, 'm, 'i, 'e, 'a, 'is, 'es, 'as_, I, E, A> {
    imports: imports::Resolution<'b, 'r, 'i, 'is, I>,
    exports: exports::Leases<'b, 'r, 'e, 'es, E>,
    answers: answers::Pipelines<'b, 'r, 'a, 'as_, A>,
    descriptor: Option<rpc_capnp::cap_descriptor::Reader<'m>>,
    exception: Option<rpc_capnp::exception::Reader<'m>>,
}
/// Resolve a promise to a remote, local or promised-answer capability. All
/// obligations and replacement ownership are preflighted before any commits.
#[allow(clippy::type_complexity)]
pub fn resolve<'b, 'r, 'm, 'i, 'e, 'a, 'is, 'es, 'as_, I, E, A>(
    resolve: rpc_capnp::resolve::Reader<'m>,
    imports: &'b mut imports::Imports<'i, 'is, I>,
    exports: &'b mut exports::Exports<'e, 'es, E>,
    answers: &'b mut answers::Answers<'a, 'as_, A>,
    slots: Slots<'r, 'i, 'e, 'a, I>,
) -> Result<Resolution<'b, 'r, 'm, 'i, 'e, 'a, 'is, 'es, 'as_, I, E, A>, Error> {
    let (descriptor, exception) = match resolve.which().map_err(|_| Error::Unsupported)? {
        rpc_capnp::resolve::Cap(descriptor) => {
            let descriptor = descriptor?;
            if slots.imports.len() != 1
                || slots.exports.len() != 1
                || slots.answers.len() != 1
                || matches!(descriptor.which(), Ok(rpc_capnp::cap_descriptor::None(())))
            {
                return Err(Error::Mismatch);
            }
            capabilities::validate_descriptor(
                descriptor,
                &slots.imports[0],
                &slots.exports[0],
                &slots.answers[0],
            )
            .map_err(Error::Reception)?;
            (Some(descriptor), None)
        }
        rpc_capnp::resolve::Exception(exception) => {
            if !slots.imports.is_empty() || !slots.exports.is_empty() || !slots.answers.is_empty() {
                return Err(Error::Mismatch);
            }
            let exception = exception?;
            exception
                .get_reason()?
                .to_str()
                .map_err(capnp::Error::from)?;
            (None, Some(exception))
        }
    };
    let imports = imports
        .prepare_resolution(ImportId::from_wire(resolve.get_promise_id()), slots.imports)
        .map_err(|e| Error::Reception(reception::Error::Imports(e)))?;
    let exports = exports
        .prepare_leases(slots.exports)
        .map_err(|e| Error::Reception(reception::Error::Exports(e)))?;
    let answers = answers
        .prepare_pipelines(slots.answers)
        .map_err(|e| Error::Reception(reception::Error::Answers(e)))?;
    Ok(Resolution {
        imports,
        exports,
        answers,
        descriptor,
        exception,
    })
}
impl<'r, 'm, 'i, 'e, 'a, I, E, A> Resolution<'_, 'r, 'm, 'i, 'e, 'a, '_, '_, '_, I, E, A> {
    pub fn accept(self) -> Resolved<'r, 'm, 'i, 'e, 'a, I> {
        let (target, imports) = self.imports.commit();
        Resolved {
            target,
            slots: Slots {
                imports,
                exports: self.exports.accept(),
                answers: self.answers.accept(),
            },
            descriptor: self.descriptor,
            exception: self.exception,
        }
    }
}
#[must_use = "transfer the replacement or release its ownership even for an abandoned promise"]
pub struct Resolved<'r, 'm, 'i, 'e, 'a, I> {
    target: Option<imports::Key<'i>>,
    slots: Slots<'r, 'i, 'e, 'a, I>,
    descriptor: Option<rpc_capnp::cap_descriptor::Reader<'m>>,
    exception: Option<rpc_capnp::exception::Reader<'m>>,
}
impl<'m, 'i, 'e, 'a, I> Resolved<'_, 'm, 'i, 'e, 'a, I> {
    pub fn target(&self) -> Option<imports::Key<'i>> {
        self.target
    }
    pub fn replacement(
        &self,
    ) -> Result<Option<capabilities::Capability<'_, 'm, 'i, 'e, 'a>>, Error> {
        self.descriptor
            .map(|d| {
                capabilities::descriptor_capability(
                    d,
                    &self.slots.imports[0],
                    &self.slots.exports[0],
                    &self.slots.answers[0],
                )
                .map_err(Error::Reception)
            })
            .transpose()
    }
    pub fn exception(&self) -> Option<rpc_capnp::exception::Reader<'m>> {
        self.exception
    }
    pub fn release<E, A>(
        self,
        imports: &mut imports::Imports<'i, '_, I>,
        exports: &mut exports::Exports<'e, '_, E>,
        answers: &mut answers::Answers<'a, '_, A>,
    ) {
        capabilities::release_slots(self.slots, imports, exports, answers);
    }
}
#[cfg(test)]
mod tests;
