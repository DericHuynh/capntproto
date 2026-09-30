//! Framing differential and fragmentation oracles, shared by libFuzzer/Cargo.
use crate::{wire, MAX_INPUT};
use capnp::message::{Reader, ReaderSegments};
use std::io::{self, BufRead, Read};

#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    Eof,
    Invalid,
    Message {
        consumed: usize,
        segments: Vec<Vec<u8>>,
    },
}

/// Bound both Read and BufRead; no buffered prefetch obscures consumption.
struct Fragmented<'a> {
    bytes: &'a [u8],
    chunk: usize,
    position: usize,
}
impl Read for Fragmented<'_> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        let n = self.fill_buf()?.len().min(output.len());
        output[..n].copy_from_slice(&self.fill_buf()?[..n]);
        self.consume(n);
        Ok(n)
    }
}
impl BufRead for Fragmented<'_> {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        Ok(&self.bytes[self.position..(self.position + self.chunk).min(self.bytes.len())])
    }
    fn consume(&mut self, n: usize) {
        assert!(n <= self.chunk.min(self.bytes.len() - self.position));
        self.position += n;
    }
}

fn segments<S: ReaderSegments>(reader: Reader<S>) -> Vec<Vec<u8>> {
    let source = reader.into_segments();
    let mut segments = Vec::new();
    for index in 0..512 {
        let Some(bytes) = source.get_segment(index) else {
            return segments;
        };
        assert_eq!(bytes.len() % 8, 0);
        segments.push(bytes.to_vec());
    }
    panic!("reader exceeded segment-count limit");
}

// Table plus body fits even at the accepted 511-segment / 512-word bounds.
#[repr(align(8))]
struct Scratch([u8; 8192]);

fn streamed(bytes: &[u8], chunk: usize, packed: bool, no_alloc: bool) -> Vec<Outcome> {
    let mut input = Fragmented {
        bytes,
        chunk,
        position: 0,
    };
    let mut scratch = Scratch([0xa5; 8192]);
    let mut outcomes = Vec::new();
    for _ in 0..2 {
        let start = input.position;
        let result = match (packed, no_alloc) {
            (false, false) => capnp::serialize::try_read_message(&mut input, wire::options())
                .map(|r| r.map(segments)),
            (false, true) => capnp::serialize::try_read_message_no_alloc(
                &mut input,
                &mut scratch.0,
                wire::options(),
            )
            .map(|r| r.map(segments)),
            (true, false) => capnp::serialize_packed::try_read_message(&mut input, wire::options())
                .map(|r| r.map(segments)),
            (true, true) => capnp::serialize_packed::try_read_message_no_alloc(
                &mut input,
                &mut scratch.0,
                wire::options(),
            )
            .map(|r| r.map(segments)),
        };
        outcomes.push(match result {
            Ok(Some(segments)) => Outcome::Message {
                consumed: input.position - start,
                segments,
            },
            Ok(None) => Outcome::Eof,
            Err(_) => Outcome::Invalid,
        });
        if !matches!(outcomes.last(), Some(Outcome::Message { .. })) {
            break;
        }
    }
    outcomes
}

/// Independent framing oracle, using checked slice ranges and wide arithmetic.
/// It validates framing only; segment contents need not be valid pointers.
fn reference(bytes: &[u8]) -> Outcome {
    if bytes.is_empty() {
        return Outcome::Eof;
    }
    let parse = || -> Option<(usize, Vec<Vec<u8>>)> {
        let count = u32::from_le_bytes(bytes.get(..4)?.try_into().ok()?).checked_add(1)? as usize;
        if count >= 512 {
            return None;
        }
        let table_bytes = (count + 2) / 2 * 8;
        let table = bytes.get(..table_bytes)?;
        let lengths: Vec<_> = (0..count)
            .map(|i| u32::from_le_bytes(table[4 + i * 4..8 + i * 4].try_into().unwrap()) as u64)
            .collect();
        if lengths.iter().sum::<u64>() > wire::WORD_LIMIT as u64 {
            return None;
        }
        let mut offset = table_bytes;
        let mut segments = Vec::new();
        for length in lengths {
            let end = offset + length as usize * 8;
            segments.push(bytes.get(offset..end)?.to_vec());
            offset = end;
        }
        Some((offset, segments))
    };
    match parse() {
        Some((consumed, segments)) => Outcome::Message { consumed, segments },
        None => Outcome::Invalid,
    }
}

fn plain(bytes: &[u8], observed: &[Outcome]) {
    // Vec<u8> does not promise word alignment. Copy into explicitly aligned
    // storage rather than weakening the production decoder's alignment checks.
    let mut aligned = Scratch([0; 8192]);
    aligned.0[..bytes.len()].copy_from_slice(bytes);
    let mut remaining = &aligned.0[..bytes.len()];
    for actual in observed {
        let expected = reference(remaining);
        assert_eq!(*actual, expected, "independent framing oracle");
        let mut allocated = remaining;
        let mut borrowed = remaining;
        let a = capnp::serialize::read_message_from_flat_slice(&mut allocated, wire::options());
        let b =
            capnp::serialize::read_message_from_flat_slice_no_alloc(&mut borrowed, wire::options());
        if let Outcome::Message {
            consumed,
            segments: expected,
        } = expected
        {
            assert_eq!(segments(a.unwrap()), expected, "flat allocated");
            assert_eq!(segments(b.unwrap()), expected, "flat borrowed");
            assert_eq!(allocated, &remaining[consumed..]);
            assert_eq!(borrowed, allocated);
            remaining = allocated;
        } else {
            assert!(a.is_err() && b.is_err());
            break;
        }
    }
}

/// Input: packed bit, fragment size minus one, then arbitrary stream bytes.
/// At most two messages are read, exposing over-consumption and stale scratch
/// bytes while keeping each invocation bounded.
pub fn check(input: &[u8]) {
    let input = &input[..input.len().min(MAX_INPUT)];
    let packed = input.first().copied().unwrap_or(0) & 1 != 0;
    let chunk = 1 + usize::from(input.get(1).copied().unwrap_or(0));
    let bytes = input.get(2..).unwrap_or_default();
    let expected = streamed(bytes, MAX_INPUT, packed, false);
    assert_eq!(
        streamed(bytes, chunk, packed, false),
        expected,
        "fragmented allocated"
    );
    assert_eq!(
        streamed(bytes, MAX_INPUT, packed, true),
        expected,
        "contiguous no-alloc"
    );
    assert_eq!(
        streamed(bytes, chunk, packed, true),
        expected,
        "fragmented no-alloc"
    );
    if !packed {
        plain(bytes, &expected);
    }
}

/// Versioned seed matrix also executed by ordinary tests. No production writer
/// participates in the expected framing or the packed corpus construction.
pub fn seeds() -> Vec<Vec<u8>> {
    let mut messages: Vec<_> = wire::shapes().iter().map(|s| wire::frame(s)).collect();
    for count in [2, 4, 5, 6, 7, 510, 511] {
        messages.push(wire::frame(&vec![vec![]; count]));
    }
    messages.push(wire::frame(&[vec![0; 256]]));
    messages.push(wire::frame(&[vec![u64::MAX; 256]]));
    let mut inputs = vec![
        vec![],
        vec![1, 0, 1, 3, 0, 1, 1, 3, 0, 1], // packed table-run regression, twice
        vec![1],
    ];
    for bytes in &messages {
        for packed in [false, true] {
            let bytes = if packed {
                wire::pack(bytes)
            } else {
                bytes.clone()
            };
            for chunk in [0, 1, 6, 7, 8, 31, 255] {
                let mut input = vec![u8::from(packed), chunk];
                input.extend_from_slice(&bytes);
                // Two adjacent messages if they fit; otherwise retain full
                // near-limit table/body input rather than silently truncating.
                if input.len() + bytes.len() <= MAX_INPUT {
                    input.extend_from_slice(&bytes);
                }
                assert!(input.len() <= MAX_INPUT);
                inputs.push(input);
            }
            // Truncations include headers, packed tags, run counts and bodies.
            for end in 0..bytes.len().min(32) {
                let mut input = vec![u8::from(packed), 2];
                input.extend_from_slice(&bytes[..end]);
                inputs.push(input);
            }
        }
    }
    for count in [511u32, 512, u32::MAX] {
        let mut input = vec![0, 0];
        input.extend_from_slice(&count.to_le_bytes());
        input.extend_from_slice(&[0; 4]);
        inputs.push(input);
    }
    let mut large = vec![0xff; MAX_INPUT];
    large[..2].copy_from_slice(&[0, 0]);
    inputs.push(large);
    inputs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oracle_rejects_wrong_segments_and_consumption() {
        let bytes = wire::frame(&[vec![0], vec![42]]);
        assert_eq!(
            reference(&bytes),
            Outcome::Message {
                consumed: 32,
                segments: vec![0u64.to_le_bytes().to_vec(), 42u64.to_le_bytes().to_vec()],
            }
        );
        for end in 1..bytes.len() {
            assert_eq!(reference(&bytes[..end]), Outcome::Invalid);
        }
        for wrong in [
            Outcome::Message {
                consumed: 31,
                segments: vec![vec![0; 8], 42u64.to_le_bytes().to_vec()],
            },
            Outcome::Message {
                consumed: 32,
                segments: vec![vec![0; 8], vec![0; 8]],
            },
        ] {
            assert!(std::panic::catch_unwind(|| plain(&bytes, &[wrong])).is_err());
        }
    }
}
