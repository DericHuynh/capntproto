//! A message round trip using only the facade's runtime re-export.
use capntproto::capnp::{message, serialize, text};

fn main() -> capntproto::capnp::Result<()> {
    let mut message = message::Builder::new_default();
    message.set_root::<text::Owned>("hello, Cap'n Proto")?;
    let bytes = serialize::write_message_to_words(&message);
    let mut input = bytes.as_slice();
    let reader = serialize::read_message_from_flat_slice(&mut input, Default::default())?;
    println!("{}", reader.get_root::<text::Reader<'_>>()?.to_str()?);
    assert!(input.is_empty());
    Ok(())
}
