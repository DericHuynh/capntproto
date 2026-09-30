//! Small, independent wire fixtures; never built with the production encoder.
use capnp::message::ReaderOptions;

pub const WORD_LIMIT: usize = 512;
pub const NESTING_LIMIT: i32 = 16;

pub fn options() -> ReaderOptions {
    ReaderOptions {
        traversal_limit_in_words: Some(WORD_LIMIT),
        nesting_limit: NESTING_LIMIT,
    }
}

pub fn frame(segments: &[Vec<u64>]) -> Vec<u8> {
    let mut bytes = ((segments.len() - 1) as u32).to_le_bytes().to_vec();
    for segment in segments {
        bytes.extend_from_slice(&(segment.len() as u32).to_le_bytes());
    }
    if segments.len().is_multiple_of(2) {
        bytes.extend_from_slice(&[0; 4]);
    }
    for segment in segments {
        for word in segment {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
    }
    bytes
}

/// Shapes include capability indices without a capability table: decoding
/// bytes alone must never grant access to a client hook.
pub fn shapes() -> Vec<Vec<Vec<u64>>> {
    let list = |size: u64, len: u64| 1 | size << 32 | len << 35;
    vec![
        vec![vec![]],
        vec![vec![0]],
        vec![vec![0xffff_fffc]], // non-null empty struct
        vec![vec![1 << 32, 42]],
        vec![vec![1 << 48, 1 << 32, 42]], // nested struct
        vec![vec![list(0, 99)]],          // zero-storage list
        vec![vec![list(1, 9), 0x155]],
        vec![vec![list(2, 3), 0x0063_6261]],
        vec![vec![list(3, 2), 0x1234_5678]],
        vec![vec![list(4, 2), 0x1234_5678_9abc_def0]],
        vec![vec![list(5, 2), 7, 8]],
        vec![vec![list(6, 2), 1 << 32 | 4, 0, 42]],
        vec![vec![list(7, 4), 8 | 1 << 32 | 1 << 48, 42, 0, 43, 0]],
        vec![vec![3]], // capability index zero
        vec![vec![3 | 99 << 32]],
        vec![vec![1 << 48, 3]],                     // nested capability
        vec![vec![2 | 1 << 32], vec![1 << 32, 42]], // far pointer
        vec![vec![6 | 1 << 32], vec![2 | 2 << 32, 1 << 32], vec![42]], // double far
        vec![vec![0xffff_fffc | 1 << 48]],          // cycle (must respect nesting limit)
        vec![vec![list(7, 0), 4 | 1 << 32]],        // composite count exceeds body
        vec![vec![2 | 99 << 32]],                   // missing far segment
        vec![vec![list(5, (1 << 29) - 1)]],         // oversized list
        vec![vec![7]],                              // reserved pointer kind
    ]
}

/// Independent packed encoder for fixtures. Runs stop at table/body boundaries.
pub fn pack(bytes: &[u8]) -> Vec<u8> {
    fn group(bytes: &[u8], output: &mut Vec<u8>) {
        let mut words = bytes.chunks_exact(8).peekable();
        while let Some(word) = words.next() {
            let tag = word
                .iter()
                .enumerate()
                .fold(0, |tag, (i, b)| tag | (u8::from(*b != 0) << i));
            output.push(tag);
            output.extend(word.iter().copied().filter(|b| *b != 0));
            if tag == 0 {
                let mut extra = 0;
                while extra < 255 && words.peek().is_some_and(|w| w.iter().all(|b| *b == 0)) {
                    words.next();
                    extra += 1;
                }
                output.push(extra);
            } else if tag == 255 {
                let mut extra = 0;
                let index = output.len();
                output.push(0);
                while extra < 255 && words.peek().is_some_and(|w| w.iter().all(|b| *b != 0)) {
                    output.extend_from_slice(words.next().unwrap());
                    extra += 1;
                }
                output[index] = extra;
            }
        }
    }
    let count = u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize + 1;
    let table = (count + 2) / 2 * 8;
    let mut output = Vec::new();
    group(&bytes[..table], &mut output);
    group(&bytes[table..], &mut output);
    output
}
