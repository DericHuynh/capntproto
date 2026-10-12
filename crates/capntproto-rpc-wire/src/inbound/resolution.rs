//! Receive a Resolve and its replacement grant without partially applying either.
use super::*;
use capntproto_rpc_engine::ids::ImportId;

#[must_use]
pub struct Resolution<'a, 'receipts, 'message, 'import, 'imports, I> {
    pending: imports::Resolution<'a, 'receipts, 'import, 'imports, I>,
    exception: Option<rpc_capnp::exception::Reader<'message>>,
}

/// Cap replacements require one matching import receipt; exceptions require
/// none. Receiver-owned replacements need a separate local lease admission.
/// A late Resolve for an absent/abandoned promise still owns its replacement:
/// accept it, then release it through Resolved::release when target() is None.
pub fn resolve<'a, 'receipts, 'message, 'import, 'imports, I>(
    resolve: rpc_capnp::resolve::Reader<'message>,
    imports: &'a mut imports::Imports<'import, 'imports, I>,
    receipts: &'receipts mut [imports::ReceiveSlot<'import, I>],
) -> Result<Resolution<'a, 'receipts, 'message, 'import, 'imports, I>, Error> {
    let exception = match resolve.which().map_err(|_| Error::Unsupported)? {
        rpc_capnp::resolve::Cap(descriptor) => {
            let descriptor = descriptor?;
            if receipts.len() != 1
                || matches!(descriptor.which(), Ok(rpc_capnp::cap_descriptor::None(())))
            {
                return Err(Error::Mismatch);
            }
            reception::check_descriptor(descriptor, &receipts[0]).map_err(Error::Reception)?;
            None
        }
        rpc_capnp::resolve::Exception(exception) => {
            if !receipts.is_empty() {
                return Err(Error::Mismatch);
            }
            let exception = exception?;
            exception
                .get_reason()?
                .to_str()
                .map_err(capnp::Error::from)?;
            Some(exception)
        }
    };
    let pending = imports
        .prepare_resolution(ImportId::from_wire(resolve.get_promise_id()), receipts)
        .map_err(|error| Error::Reception(reception::Error::Imports(error)))?;
    Ok(Resolution { pending, exception })
}
impl<'receipts, 'message, 'import, I> Resolution<'_, 'receipts, 'message, 'import, '_, I> {
    pub fn accept(self) -> Resolved<'receipts, 'message, 'import, I> {
        let (target, receipts) = self.pending.commit();
        Resolved {
            target,
            receipts,
            exception: self.exception,
        }
    }
}

#[must_use = "transfer the replacement or explicitly release it"]
pub struct Resolved<'receipts, 'message, 'import, I> {
    target: Option<imports::Key<'import>>,
    receipts: &'receipts mut [imports::ReceiveSlot<'import, I>],
    exception: Option<rpc_capnp::exception::Reader<'message>>,
}
impl<'message, 'import, I> Resolved<'_, 'message, 'import, I> {
    pub fn target(&self) -> Option<imports::Key<'import>> {
        self.target
    }
    pub fn replacement(&self) -> Option<&imports::Local<'import>> {
        self.receipts.first().and_then(|r| r.local())
    }
    pub fn exception(&self) -> Option<rpc_capnp::exception::Reader<'message>> {
        self.exception
    }
    pub fn release(self, imports: &mut imports::Imports<'import, '_, I>) {
        for receipt in self.receipts {
            if let Some(local) = receipt.take_local() {
                imports.release(local);
            }
        }
    }
}

#[cfg(test)]
mod tests;
