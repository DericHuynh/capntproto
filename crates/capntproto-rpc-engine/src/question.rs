//! Outgoing question lifecycle, independent of promises, tables and transport.
//! Callers retain the record until both the local reference and remote Return
//! are gone. Cancellation does not authorize reusing an outstanding wire ID.

#[derive(Debug)]
enum State {
    Awaiting,
    Cancelled,
    Returned { finish: bool },
    Released,
}

#[derive(Debug)]
pub struct Question {
    state: State,
}

/// Required Finish effect. The caller must retain this effect until accepted
/// by its output adapter, including when ordinary call admission is full.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Finish {
    pub release_result_caps: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Release {
    pub finish: Option<Finish>,
    pub reclaim: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delivery {
    Deliver,
    /// The caller cancelled before Return. Ignore its result and reclaim the
    /// record; the previously emitted Finish releases its result capabilities.
    Discard,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DuplicateReturn;

impl core::fmt::Display for DuplicateReturn {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("duplicate Return or Return for a no-return question")
    }
}
impl core::error::Error for DuplicateReturn {}

impl Default for Question {
    fn default() -> Self {
        Self::new()
    }
}

impl Question {
    pub const fn new() -> Self {
        Self {
            state: State::Awaiting,
        }
    }

    /// Provide and pipeline-only questions require Finish but have no Return.
    pub const fn no_return() -> Self {
        Self {
            state: State::Returned { finish: true },
        }
    }

    pub fn is_awaiting_return(&self) -> bool {
        matches!(self.state, State::Awaiting | State::Cancelled)
    }

    /// The peer still retains this answer for promised-answer references.
    /// A noFinishNeeded Return permits it to discard that answer immediately.
    pub fn can_pipeline(&self) -> bool {
        matches!(
            self.state,
            State::Awaiting | State::Returned { finish: true }
        )
    }

    /// Mutation-free input preflight. The caller must keep the lifecycle
    /// exclusively borrowed until the matching validated Return commits.
    pub fn return_delivery(&self) -> Result<Delivery, DuplicateReturn> {
        match self.state {
            State::Awaiting => Ok(Delivery::Deliver),
            State::Cancelled => Ok(Delivery::Discard),
            _ => Err(DuplicateReturn),
        }
    }

    /// Apply a validated Return (or an Unimplemented Join, with no Finish).
    /// Duplicate/forbidden returns do not mutate state. Payload validation and
    /// method-specific noFinishNeeded restrictions remain the caller's job.
    pub fn returned(&mut self, no_finish_needed: bool) -> Result<Delivery, DuplicateReturn> {
        let delivery = self.return_delivery()?;
        match delivery {
            Delivery::Deliver => {
                self.state = State::Returned {
                    finish: !no_finish_needed,
                };
            }
            Delivery::Discard => {
                self.state = State::Released;
            }
        }
        Ok(delivery)
    }

    /// Release the last local reference once. Repeated releases cannot emit a
    /// second Finish. The record remains occupied after early cancellation.
    pub fn release(&mut self) -> Release {
        let (finish, reclaim) = match self.state {
            State::Awaiting => {
                self.state = State::Cancelled;
                (
                    Some(Finish {
                        release_result_caps: true,
                    }),
                    false,
                )
            }
            State::Cancelled => (None, false),
            State::Returned { finish } => {
                self.state = State::Released;
                (
                    finish.then_some(Finish {
                        release_result_caps: false,
                    }),
                    true,
                )
            }
            State::Released => (None, true),
        };
        Release { finish, reclaim }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_keeps_the_wire_id_until_the_late_return() {
        for no_finish in [false, true] {
            let mut question = Question::new();
            assert_eq!(
                question.release(),
                Release {
                    finish: Some(Finish {
                        release_result_caps: true
                    }),
                    reclaim: false
                }
            );
            assert_eq!(
                question.release(),
                Release {
                    finish: None,
                    reclaim: false
                }
            );
            assert!(question.is_awaiting_return());
            assert_eq!(question.returned(no_finish), Ok(Delivery::Discard));
            assert_eq!(question.returned(no_finish), Err(DuplicateReturn));
            assert_eq!(
                question.release(),
                Release {
                    finish: None,
                    reclaim: true
                }
            );
        }
    }

    #[test]
    fn retained_results_finish_once_and_never_release_imported_caps_twice() {
        for no_finish in [false, true] {
            let mut question = Question::new();
            assert_eq!(question.returned(no_finish), Ok(Delivery::Deliver));
            assert_eq!(question.returned(!no_finish), Err(DuplicateReturn));
            assert_eq!(
                question.release(),
                Release {
                    finish: (!no_finish).then_some(Finish {
                        release_result_caps: false
                    }),
                    reclaim: true
                }
            );
            assert_eq!(
                question.release(),
                Release {
                    finish: None,
                    reclaim: true
                }
            );
        }
    }

    #[test]
    fn no_return_questions_reject_return_without_losing_their_finish() {
        let mut question = Question::no_return();
        assert_eq!(question.returned(true), Err(DuplicateReturn));
        assert_eq!(
            question.release(),
            Release {
                finish: Some(Finish {
                    release_result_caps: false
                }),
                reclaim: true
            }
        );
        assert_eq!(
            question.release(),
            Release {
                finish: None,
                reclaim: true
            }
        );
    }
}
