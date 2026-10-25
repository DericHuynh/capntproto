//! Proof identity shared by immutable hosted Call and Return publication.
//! Descriptor authority and resource ownership are still checked at publication.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Validation {
    traversal: usize,
    capabilities: u32,
    // Successful ancestor validation requires a positive i32 nesting limit.
    // Its unused sign bit records unlimited traversal, preserving every usize
    // limit (including usize::MAX) distinctly from None. Nonzero gives Option
    // its absent value without storing another discriminant in each request.
    policy: core::num::NonZeroU32,
}
impl Validation {
    pub(super) fn new(capabilities: u32, options: capnp::message::ReaderOptions) -> Option<Self> {
        if options.nesting_limit <= 0 {
            return None;
        }
        let policy = options.nesting_limit as u32
            | (u32::from(options.traversal_limit_in_words.is_none()) << 31);
        Some(Self {
            capabilities,
            traversal: options.traversal_limit_in_words.unwrap_or(0),
            policy: core::num::NonZeroU32::new(policy).expect("positive nesting limit"),
        })
    }
    pub(super) fn matches(self, capabilities: u32, options: capnp::message::ReaderOptions) -> bool {
        Some(self) == Self::new(capabilities, options)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn proof_identity_distinguishes_unlimited_and_maximum_finite_limits() {
        let mut policies = alloc::vec::Vec::new();
        for capabilities in [0, 1, u32::MAX] {
            for nesting_limit in [1, 3, 64, i32::MAX] {
                for traversal_limit_in_words in [None, Some(0), Some(1), Some(usize::MAX)] {
                    let options = capnp::message::ReaderOptions {
                        nesting_limit,
                        traversal_limit_in_words,
                    };
                    let proof = Validation::new(capabilities, options).unwrap();
                    assert!(proof.matches(capabilities, options));
                    assert!(policies.iter().all(|previous| *previous != proof));
                    assert!(!proof.matches(
                        capabilities,
                        capnp::message::ReaderOptions {
                            nesting_limit: 0,
                            ..options
                        }
                    ));
                    assert!(!proof.matches(
                        capabilities,
                        capnp::message::ReaderOptions {
                            nesting_limit: -1,
                            ..options
                        }
                    ));
                    policies.push(proof);
                }
            }
        }
    }
}
