use super::*;

fn fixture() -> Vec<u8> {
    let mut bytes = 3u64.to_le_bytes().to_vec();
    for (key, revision, publish, payload) in [
        (1, 2, 1, &b"abc"[..]),
        (2, 5, 0, &b"012345678"[..]),
        (3, 1, 1, &b""[..]),
    ] {
        for word in [key, revision, publish, payload.len() as u64] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        bytes.extend_from_slice(payload);
        while !bytes.len().is_multiple_of(8) {
            bytes.push(0);
        }
    }
    bytes
}

#[test]
fn checked_batch_headers_preserve_offsets_and_reject_truncation() {
    let bytes = fixture();
    let entries = batch_entries(&bytes).unwrap();
    assert_eq!(entries.len(), 3);
    for (entry, (key, revision, publish, payload)) in entries.iter().zip([
        (1, 2, true, &b"abc"[..]),
        (2, 5, false, &b"012345678"[..]),
        (3, 1, true, &b""[..]),
    ]) {
        assert_eq!(
            (entry.object.get(), entry.revision.get(), entry.publish),
            (key, revision, publish)
        );
        assert_eq!(&bytes[entry.offset..entry.offset + entry.len], payload);
    }
    for end in 0..bytes.len() {
        assert!(batch_entries(&bytes[..end]).is_err(), "truncated at {end}");
    }
}

#[test]
fn checked_batch_headers_reject_invalid_coordinates_padding_and_overflow() {
    let original = fixture();
    for (offset, word) in [(0, 0), (0, 17), (16, 0), (24, 2), (32, u64::MAX), (48, 1)] {
        let mut bytes = original.clone();
        bytes[offset..offset + 8].copy_from_slice(&word.to_le_bytes());
        assert!(
            batch_entries(&bytes).is_err(),
            "offset={offset}, word={word}"
        );
    }
    let mut bytes = original.clone();
    bytes[43] = 1;
    assert!(batch_entries(&bytes).is_err());
    let mut bytes = original;
    bytes.push(0);
    assert!(batch_entries(&bytes).is_err());
}
