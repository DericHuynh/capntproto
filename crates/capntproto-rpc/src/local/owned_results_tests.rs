use super::*;

fn response() -> Rc<dyn ResponseHook> {
    let mut message = message::Builder::new_default();
    message
        .set_root::<capnp::data::Owned>(&[0x5a; 65536][..])
        .unwrap();
    Rc::new(ResultsDone::new(message, Vec::new()))
}

fn results() -> (Results, oneshot::Receiver<Box<dyn ResultsDoneHook>>) {
    let (sender, receiver) = oneshot::channel();
    let (pipeline, _) = crate::queued::Pipeline::new();
    (Results::new(sender, pipeline), receiver)
}

#[test]
fn adopted_response_shares_bytes_and_retains_context_through_readers_and_guard() {
    let response = response();
    let original = response
        .get()
        .unwrap()
        .get_as::<capnp::data::Reader>()
        .unwrap()
        .as_ptr();
    let (mut results, receiver) = results();
    let context = Rc::new(());
    let weak = Rc::downgrade(&context);
    results.retain_response_context(context);
    let guard = results.cancellation_guard().unwrap();
    results.set_response(response).unwrap();
    assert!(results.message.is_none());
    drop(results);
    let reader = futures::executor::block_on(receiver).unwrap();
    assert_eq!(
        reader
            .get()
            .unwrap()
            .get_as::<capnp::data::Reader>()
            .unwrap()
            .as_ptr(),
        original
    );
    let retained = reader.add_ref();
    drop(reader);
    assert!(weak.upgrade().is_some());
    drop(retained);
    assert!(weak.upgrade().is_some());
    drop(guard);
    assert!(weak.upgrade().is_none());
}

#[test]
fn editing_adopted_results_copies_without_mutating_the_original_response() {
    let response = response();
    let (mut results, receiver) = results();
    results.set_response(response.clone()).unwrap();
    results
        .get()
        .unwrap()
        .get_as::<capnp::data::Builder>()
        .unwrap()[0] = 7;
    assert_eq!(
        response
            .get()
            .unwrap()
            .get_as::<capnp::data::Reader>()
            .unwrap()[0],
        0x5a
    );
    drop(results);
    let reader = futures::executor::block_on(receiver).unwrap();
    assert_eq!(
        reader
            .get()
            .unwrap()
            .get_as::<capnp::data::Reader>()
            .unwrap()[0],
        7
    );
}

#[test]
fn rejected_response_preserves_the_previous_results() {
    struct Broken;
    impl ResponseHook for Broken {
        fn get(&self) -> capnp::Result<any_pointer::Reader<'_>> {
            Err(Error::failed("injected response failure".into()))
        }
    }
    let (mut results, receiver) = results();
    results.set_response(response()).unwrap();
    assert_eq!(
        results.set_response(Rc::new(Broken)).unwrap_err().extra,
        "injected response failure"
    );
    drop(results);
    let reader = futures::executor::block_on(receiver).unwrap();
    assert_eq!(
        reader
            .get()
            .unwrap()
            .get_as::<capnp::data::Reader>()
            .unwrap(),
        &[0x5a; 65536][..]
    );
}

struct Wrapper {
    inner: Results,
    accesses: Rc<std::cell::Cell<usize>>,
}
impl ResultsHook for Wrapper {
    fn get_with_size_hint(
        &mut self,
        hint: Option<capnp::MessageSize>,
    ) -> capnp::Result<any_pointer::Builder<'_>> {
        self.accesses.set(self.accesses.get() + 1);
        self.inner.get_with_size_hint(hint)
    }
    fn set_pipeline(&mut self) -> capnp::Result<()> {
        self.inner.set_pipeline()
    }
    fn set_pipeline_from(&mut self, pipeline: Box<dyn PipelineHook>) -> capnp::Result<()> {
        self.inner.set_pipeline_from(pipeline)
    }
    fn allow_cancellation(&self) {}
    fn tail_call(self: Box<Self>, request: Box<dyn RequestHook>) -> Promise<(), Error> {
        Box::new(self.inner).tail_call(request)
    }
    fn direct_tail_call(
        self: Box<Self>,
        request: Box<dyn RequestHook>,
    ) -> (Promise<(), Error>, Box<dyn PipelineHook>) {
        Box::new(self.inner).direct_tail_call(request)
    }
}

#[test]
fn default_response_adoption_cannot_bypass_a_wrapper_builder() {
    let response = response();
    let (inner, receiver) = results();
    let accesses = Rc::new(std::cell::Cell::new(0));
    let mut wrapper = Wrapper {
        inner,
        accesses: accesses.clone(),
    };
    wrapper.set_response(response.clone()).unwrap();
    assert_eq!(accesses.get(), 1);
    assert!(wrapper.inner.response.is_none());
    drop(wrapper);
    let reader = futures::executor::block_on(receiver).unwrap();
    assert_eq!(
        reader
            .get()
            .unwrap()
            .get_as::<capnp::data::Reader>()
            .unwrap(),
        response
            .get()
            .unwrap()
            .get_as::<capnp::data::Reader>()
            .unwrap()
    );
}
