//! Byte-window accounting with caller-supplied acknowledgement times.
//!
//! This layer does not send messages, retain futures, or wake tasks. A driver
//! owns each receipt until its application acknowledgement and observes
//! [`Window::is_ready`] to release blocked senders.
use core::time::Duration;

mod adaptive;
pub mod incoming;
pub mod outgoing;
use adaptive::{AdaptiveWindow, Snapshot};

/// A resource or clock input could not be applied. No strings are allocated.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    CounterOverflow,
    InvalidAcknowledgement,
    ClockWentBackwards,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::CounterOverflow => "RPC streaming byte counter overflow",
            Self::InvalidAcknowledgement => "invalid RPC streaming acknowledgement",
            Self::ClockWentBackwards => "RPC streaming clock went backwards",
        })
    }
}
impl core::error::Error for Error {}

/// One send's accounting, consumed by its acknowledgement. Not a delivery receipt.
/// Receipts must be returned to the window which issued them.
#[derive(Debug)]
#[must_use = "retain the receipt until acknowledgement or stream failure"]
pub struct Receipt {
    size: u128,
    snapshot: Option<Snapshot>,
}

/// Streaming credit state. The adapter owns pending calls and terminal errors.
pub struct Window {
    size: usize,
    in_flight: u128,
    largest: u128,
    adaptive: Option<AdaptiveWindow>,
    last_time: Option<Duration>,
}

impl Window {
    pub fn fixed(size: usize) -> Self {
        Self {
            size,
            in_flight: 0,
            largest: 0,
            adaptive: None,
            last_time: None,
        }
    }

    pub fn adaptive(size: usize) -> Self {
        Self {
            adaptive: Some(AdaptiveWindow::new()),
            ..Self::fixed(size)
        }
    }

    pub fn size(&self) -> usize {
        self.size
    }
    pub fn in_flight(&self) -> u128 {
        self.in_flight
    }

    /// The driver may skip an expensive socket-window query while false.
    pub fn needs_window(&self) -> bool {
        self.in_flight > self.largest
    }

    pub fn set_size(&mut self, size: usize) {
        self.size = size;
    }

    pub fn is_ready(&self) -> bool {
        (self.adaptive.is_none() && self.in_flight <= self.largest)
            || self.in_flight < self.largest
            || self.in_flight - self.largest < self.size as u128
    }

    /// Charge a send before querying an optional external window getter. Failing
    /// admission leaves credit unchanged. `now` is used only by adaptive mode.
    pub fn sent(&mut self, size: u128, now: Duration) -> Result<Receipt, Error> {
        self.check_time(now)?;
        let total = self
            .in_flight
            .checked_add(size)
            .ok_or(Error::CounterOverflow)?;
        self.in_flight = total;
        self.largest = self.largest.max(size);
        if self.adaptive.is_some() {
            self.last_time = Some(now);
        }
        Ok(Receipt {
            size,
            snapshot: self
                .adaptive
                .as_ref()
                .map(|adaptive| adaptive.sent(now, size, self.size, !self.is_ready())),
        })
    }

    /// A successful application acknowledgement releases credit. A rejected
    /// acknowledgement must cause the adapter to fail the stream.
    pub fn ack(&mut self, receipt: Receipt, now: Duration) -> Result<(), Error> {
        self.check_time(now)?;
        let remaining = self
            .in_flight
            .checked_sub(receipt.size)
            .ok_or(Error::InvalidAcknowledgement)?;
        if let Some(snapshot) = receipt.snapshot {
            let adaptive = self
                .adaptive
                .as_mut()
                .ok_or(Error::InvalidAcknowledgement)?;
            self.size = adaptive.ack(snapshot, now, self.size)?;
        }
        self.in_flight = remaining;
        if self.adaptive.is_some() {
            self.last_time = Some(now);
        }
        Ok(())
    }

    fn check_time(&self, now: Duration) -> Result<(), Error> {
        if self.last_time.is_some_and(|last| now < last) {
            Err(Error::ClockWentBackwards)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oversized_message_retains_forward_progress() {
        let mut credit = Window::fixed(16);
        let first = credit.sent(100, Duration::ZERO).unwrap();
        assert!(credit.is_ready());
        let second = credit.sent(100, Duration::ZERO).unwrap();
        assert!(!credit.is_ready());
        credit.ack(first, Duration::ZERO).unwrap();
        assert!(credit.is_ready());
        credit.ack(second, Duration::ZERO).unwrap();
        assert_eq!(credit.in_flight(), 0);
    }

    #[test]
    fn overflow_rejects_admission_without_changing_credit() {
        let mut credit = Window::fixed(0);
        let receipt = credit.sent(u128::MAX, Duration::ZERO).unwrap();
        assert_eq!(
            credit.sent(1, Duration::ZERO).unwrap_err(),
            Error::CounterOverflow
        );
        assert_eq!(credit.in_flight(), u128::MAX);
        credit.ack(receipt, Duration::ZERO).unwrap();
        assert_eq!(credit.in_flight(), 0);
    }

    #[test]
    fn backwards_time_does_not_release_credit() {
        let mut credit = Window::adaptive(64 * 1024);
        let receipt = credit.sent(32, Duration::from_secs(10)).unwrap();
        assert_eq!(
            credit.ack(receipt, Duration::from_secs(9)),
            Err(Error::ClockWentBackwards)
        );
        assert_eq!(credit.in_flight(), 32);
        assert_eq!(credit.size(), 64 * 1024);
    }

    #[test]
    fn equal_timestamps_supply_no_bandwidth_estimate() {
        let mut credit = Window::adaptive(1234);
        for _ in 0..3 {
            let receipt = credit.sent(16, Duration::ZERO).unwrap();
            credit.ack(receipt, Duration::ZERO).unwrap();
            assert_eq!(credit.size(), 1234);
        }
    }

    #[test]
    fn clock_fault_between_sends_preserves_original_receipt() {
        let mut credit = Window::adaptive(16);
        let first = credit.sent(32, Duration::from_secs(10)).unwrap();
        assert_eq!(
            credit.sent(64, Duration::from_secs(9)).unwrap_err(),
            Error::ClockWentBackwards
        );
        assert_eq!(credit.in_flight(), 32);
        credit.ack(first, Duration::from_secs(11)).unwrap();
        assert_eq!(credit.in_flight(), 0);
    }
}
