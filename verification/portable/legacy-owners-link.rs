//! Owner-parameterized legacy parameter/response reads without Box or an allocator.
#![cfg_attr(target_os = "none", no_std)]
#![cfg_attr(target_os = "none", no_main)]
use capnp::{
    any_pointer,
    capability::{Params, Response},
    message, text, Error, ErrorKind,
};
use core::cell::Cell;
struct Frame<'a> {
    bytes: &'a [u8],
    dropped: &'a Cell<u8>,
}
impl message::ReaderSegments for Frame<'_> {
    fn get_segment(&self, index: u32) -> Option<&[u8]> {
        (index == 0).then_some(self.bytes)
    }
    fn len(&self) -> usize {
        1
    }
}
impl Drop for Frame<'_> {
    fn drop(&mut self) {
        self.dropped.set(self.dropped.get() + 1);
    }
}
fn exercise() {
    let mut words = [capnp::word(0, 0, 0, 0, 0, 0, 0, 0); 4];
    let mut builder = message::FixedBuilder::new(&mut words).unwrap();
    builder.try_set_root_text("legacy").unwrap();
    let bytes = builder.into_segment();
    let dropped = Cell::new(0);
    let mut options = message::ReaderOptions::new();
    options.traversal_limit_in_words(Some(3));
    let reader = message::Reader::new(
        Frame {
            bytes,
            dropped: &dropped,
        },
        options,
    );
    let (selected, ()) = reader
        .try_select_pointer::<_, Error>(|root| Ok(Some((root, ()))))
        .unwrap()
        .unwrap();
    let params = Params::<any_pointer::Owned, _>::from_owner(selected).into_typed::<text::Owned>();
    let view = params.get().unwrap();
    assert_eq!(view, "legacy");
    assert!(view.as_bytes().as_ptr().addr() >= bytes.as_ptr().addr());
    assert!(view.as_bytes().as_ptr().addr() + view.len() <= bytes.as_ptr().addr() + bytes.len());
    let response = Response::<text::Owned, _>::from_owner(params.hook);
    assert_eq!(dropped.get(), 0);
    assert_eq!(response.get().unwrap(), "legacy");
    assert_eq!(
        response.get().unwrap_err().kind,
        ErrorKind::ReadLimitExceeded
    );
    drop(response);
    assert_eq!(dropped.get(), 1);
}
#[cfg(not(target_os = "none"))]
fn main() {
    exercise();
}
#[cfg(target_os = "none")]
#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    exercise();
    loop {
        core::hint::spin_loop();
    }
}
#[cfg(target_os = "none")]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    loop {
        core::hint::spin_loop();
    }
}
