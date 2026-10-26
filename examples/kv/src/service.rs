use capntproto::{capnp, capnp_rpc};
use capntproto_kv_example::key_value_capnp::directory;
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

#[derive(Default)]
pub struct Directory {
    values: RefCell<BTreeMap<String, Vec<u8>>>,
    children: RefCell<BTreeMap<String, Rc<Directory>>>,
}

fn name(value: capnp::text::Reader<'_>) -> capnp::Result<&str> {
    let value = value.to_str()?;
    if value.is_empty() || value.len() > 256 || value.contains('/') || matches!(value, "." | "..") {
        return Err(capnp::Error::failed(
            "expected one nonempty path component (at most 256 bytes)".into(),
        ));
    }
    Ok(value)
}

impl directory::Server for Directory {
    fn open(
        self: Rc<Self>,
        context: directory::OpenContext<'_>,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        context.complete(move |params, mut results| {
            let name = name(params.get_name()?)?;
            let child = self
                .children
                .borrow_mut()
                .entry(name.into())
                .or_default()
                .clone();
            results.set_directory(capnp_rpc::new_client_from_rc(child));
            Ok(())
        })
    }

    fn put(
        self: Rc<Self>,
        context: directory::PutContext<'_>,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        context.complete(move |params, _| {
            let key = name(params.get_key()?)?;
            let value = params.get_value()?;
            if value.len() > 1024 * 1024 {
                return Err(capnp::Error::overloaded(
                    "values are limited to 1 MiB".into(),
                ));
            }
            self.values.borrow_mut().insert(key.into(), value.into());
            Ok(())
        })
    }

    fn get(
        self: Rc<Self>,
        context: directory::GetContext<'_>,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        context.complete(move |params, mut results| {
            let key = name(params.get_key()?)?;
            if let Some(value) = self.values.borrow().get(key) {
                results.set_found(true);
                results.set_value(value);
            }
            Ok(())
        })
    }

    fn delete(
        self: Rc<Self>,
        context: directory::DeleteContext<'_>,
    ) -> capnp::capability::Promise<(), capnp::Error> {
        context.complete(move |params, mut results| {
            results.set_deleted(
                self.values
                    .borrow_mut()
                    .remove(name(params.get_key()?)?)
                    .is_some(),
            );
            Ok(())
        })
    }
}
