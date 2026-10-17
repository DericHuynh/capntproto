//! Upstream generated service bindings for the shared benchmark operations.
use super::*;
impl workflows::Server for Service {
    async fn echo(
        self: Rc<Self>,
        params: workflows::EchoParams,
        mut results: workflows::EchoResults,
    ) -> capnp::Result<()> {
        let params = params.get()?;
        let mut results = results.get();
        results.set_sequence(params.get_sequence());
        results.set_payload(params.get_payload()?);
        Ok(())
    }
    async fn open(
        self: Rc<Self>,
        params: workflows::OpenParams,
        mut results: workflows::OpenResults,
    ) -> capnp::Result<()> {
        let client: cell::Client = capnp_rpc::new_client(Cell(RefCell::new((
            0,
            params.get()?.get_payload()?.to_vec(),
        ))));
        results.get().set_cell(client);
        Ok(())
    }
    async fn callback(
        self: Rc<Self>,
        params: workflows::CallbackParams,
        mut results: workflows::CallbackResults,
    ) -> capnp::Result<()> {
        let params = params.get()?;
        let mut request = params.get_sink()?.notify_request();
        request.get().set_sequence(params.get_sequence());
        request.get().set_payload(params.get_payload()?);
        let response = request.send().promise.await?;
        results.get().set_sequence(response.get()?.get_sequence());
        Ok(())
    }
}
impl cell::Server for Cell {
    async fn read(
        self: Rc<Self>,
        _: cell::ReadParams,
        mut results: cell::ReadResults,
    ) -> capnp::Result<()> {
        let state = self.0.borrow();
        let mut results = results.get();
        results.set_version(state.0);
        results.set_payload(&state.1);
        Ok(())
    }
    async fn write(
        self: Rc<Self>,
        params: cell::WriteParams,
        mut results: cell::WriteResults,
    ) -> capnp::Result<()> {
        let params = params.get()?;
        let mut state = self.0.borrow_mut();
        if params.get_expected_version() != state.0 {
            return Err(capnp::Error::failed("version mismatch".into()));
        }
        state.0 = state
            .0
            .checked_add(1)
            .ok_or_else(|| capnp::Error::failed("version overflow".into()))?;
        state.1 = params.get_payload()?.to_vec();
        results.get().set_version(state.0);
        Ok(())
    }
}
impl callback::Server for Sink {
    async fn notify(
        self: Rc<Self>,
        params: callback::NotifyParams,
        mut results: callback::NotifyResults,
    ) -> capnp::Result<()> {
        let params = params.get()?;
        if params.get_payload()? != self.payload || params.get_sequence() != self.calls.get() {
            return Err(capnp::Error::failed("callback payload mismatch".into()));
        }
        self.calls.set(self.calls.get() + 1);
        results.get().set_sequence(params.get_sequence());
        Ok(())
    }
}
