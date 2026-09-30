//! Bounded malformed-pointer traversal and copy/canonicalization properties.
use crate::{wire, MAX_INPUT};
use capnp::{any_list, any_pointer, any_struct, message, Equality, Word};

fn walk(pointer: any_pointer::Reader<'_>, remaining: &mut usize) {
    if *remaining == 0 {
        return;
    }
    *remaining -= 1;
    let _ = pointer.get_pointer_type();
    // There is deliberately no capability table. Wire indices alone must not
    // fabricate authority, including when nested inside structs/lists.
    assert!(pointer
        .get_as_capability::<capnp::capability::Client>()
        .is_err());
    if let Ok(text) = pointer.get_as::<capnp::text::Reader>() {
        let _ = text.to_str();
    }
    if let Ok(value) = pointer.get_as::<any_struct::Reader>() {
        std::hint::black_box(value.get_data_section());
        for child in value.get_pointer_section().iter().take(16).flatten() {
            walk(child, remaining);
        }
    }
    if let Ok(value) = pointer.get_as::<any_list::Reader>() {
        let _ = value.get_raw_bytes();
        if let Ok(list) = value.get_as_struct_list() {
            for element in list.iter().take(16).flatten() {
                std::hint::black_box(element.get_data_section());
                for child in element.get_pointer_section().iter().take(16).flatten() {
                    walk(child, remaining);
                }
            }
        }
        if let Ok(list) = value.get_as::<capnp::primitive_list::Owned<u64>>() {
            for value in list.iter().take(16) {
                std::hint::black_box(value);
            }
        }
    }
}

/// Input: operation (raw/XOR/overwrite/truncate), fixture index, little-endian
/// mutation offset, then bytes. Mutations span tables, tags, offsets and bodies.
pub fn check(input: &[u8]) {
    let input = &input[..input.len().min(MAX_INPUT)];
    let mode = input.first().copied().unwrap_or(0) % 4;
    let body = input.get(4..).unwrap_or_default();
    let bytes = if mode == 0 {
        body.to_vec()
    } else {
        let shapes = wire::shapes();
        let shape = input.get(1).copied().unwrap_or(0) as usize % shapes.len();
        let mut bytes = wire::frame(&shapes[shape]);
        let offset = u16::from_le_bytes([
            input.get(2).copied().unwrap_or(0),
            input.get(3).copied().unwrap_or(0),
        ]) as usize;
        if mode == 3 {
            bytes.truncate(offset % (bytes.len() + 1));
        } else {
            for (index, byte) in body.iter().enumerate() {
                let position = (offset + index) % bytes.len();
                if mode == 1 {
                    bytes[position] ^= byte;
                } else {
                    bytes[position] = *byte;
                }
            }
        }
        bytes
    };
    check_wire(&bytes);
}

fn check_wire(bytes: &[u8]) {
    let Ok(message) = capnp::serialize::read_message(bytes, wire::options()) else {
        return;
    };
    let segments = message.into_segments();
    // Traversal accounting is stateful. Each independent operation needs its
    // own reader, so a previous error cannot exhaust its neighbor's budget.
    let reader = || message::Reader::new(&segments, wire::options());
    let source = reader();
    let Ok(root) = source.get_root::<any_pointer::Reader>() else {
        return;
    };
    walk(root, &mut 64);
    let source = reader();
    let _ = source
        .get_root::<any_pointer::Reader>()
        .unwrap()
        .target_size();
    let source = reader();
    let other = reader();
    if let Ok(equal) = source
        .get_root::<any_pointer::Reader>()
        .unwrap()
        .equals(other.get_root().unwrap())
    {
        assert_ne!(equal, Equality::NotEqual, "reflexivity");
    }
    let source = reader();
    let mut copied = message::Builder::new_default();
    if copied
        .set_root::<any_pointer::Owned>(source.get_root::<any_pointer::Reader>().unwrap())
        .is_ok()
    {
        let source = reader();
        let result = source
            .get_root::<any_pointer::Reader>()
            .unwrap()
            .equals(copied.get_root_as_reader().unwrap());
        if let Ok(equal) = result {
            assert_eq!(
                equal,
                Equality::Equal,
                "copy changed content or granted authority"
            );
        }
    }
    let source = reader();
    let _ = source.is_canonical();
    let source = reader();
    if let Ok(canonical) = source.canonicalize() {
        let bytes = Word::words_to_bytes(&canonical);
        let canonical_reader = || message::Reader::new(vec![bytes], wire::options());
        assert!(
            canonical_reader().is_canonical().unwrap(),
            "output is not canonical"
        );
        assert_eq!(
            Word::words_to_bytes(&canonical_reader().canonicalize().unwrap()),
            bytes,
            "canonicalization is not idempotent",
        );
    }
}

pub fn seeds() -> Vec<Vec<u8>> {
    let mut seeds = vec![vec![], vec![1, 17, 0, 0], vec![1]];
    for (index, shape) in wire::shapes().iter().enumerate() {
        let bytes = wire::frame(shape);
        let mut raw = vec![0; 4];
        raw.extend_from_slice(&bytes);
        seeds.push(raw);
        seeds.push(vec![1, index as u8, 0, 0]);
        for offset in 0..bytes.len() {
            let [low, high] = (offset as u16).to_le_bytes();
            for mode in [1, 2, 3] {
                seeds.push(vec![mode, index as u8, low, high, 0xff]);
            }
        }
    }
    let mut maximum = vec![0xff; MAX_INPUT];
    maximum[..4].copy_from_slice(&[1, 17, 0, 0]);
    seeds.push(maximum);
    seeds
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capabilities_and_cycles_have_explicit_expected_outcomes() {
        for index in [0, 99, u32::MAX] {
            let frame = wire::frame(&[vec![3 | u64::from(index) << 32]]);
            let source = capnp::serialize::read_message(frame.as_slice(), wire::options()).unwrap();
            let root = source.get_root::<any_pointer::Reader>().unwrap();
            assert_eq!(root.target_size().unwrap().cap_count, 1);
            assert_eq!(
                root.equals(root).unwrap(),
                Equality::UnknownContainsCapabilities
            );
            assert!(root
                .get_as_capability::<capnp::capability::Client>()
                .is_err());
            assert!(source.canonicalize().is_err());
        }
        let cycle = wire::frame(&[vec![0xffff_fffc | 1 << 48]]);
        let source = capnp::serialize::read_message(cycle.as_slice(), wire::options()).unwrap();
        assert!(source
            .get_root::<any_pointer::Reader>()
            .unwrap()
            .target_size()
            .is_err());
        assert!(source.canonicalize().is_err());
    }
}
