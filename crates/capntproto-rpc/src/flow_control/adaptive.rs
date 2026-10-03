// Copyright (c) 2013-2014 Sandstorm Development Group, Inc. and contributors
// SPDX-License-Identifier: MIT
// Port of AdaptiveFlowController in the pinned Cap'n Proto rpc.c++.
use std::time::Duration;

pub(super) const MIN_WINDOW: usize = 64 * 1024;
pub(super) const MAX_WINDOW: usize = 1024 * 1024 * 1024;

#[derive(Clone, Copy)]
pub(super) struct Snapshot {
    sent: Duration,
    size: u128,
    delivered: u128,
    delivered_time: Option<Duration>,
    window: usize,
    full: bool,
}

pub(super) struct AdaptiveWindow {
    delivered: u128,
    delivered_time: Option<Duration>,
    first_ack: Option<(Duration, u128)>,
    min_rtt: Duration,
    startup: bool,
    plateau_rounds: u8,
    last_round_window: usize,
    round_start: Option<Duration>,
}

impl AdaptiveWindow {
    pub(super) fn new() -> Self {
        Self {
            delivered: 0,
            delivered_time: None,
            first_ack: None,
            min_rtt: Duration::from_secs(365 * 24 * 60 * 60),
            startup: true,
            plateau_rounds: 0,
            last_round_window: 0,
            round_start: None,
        }
    }

    pub(super) fn sent(&self, now: Duration, size: u128, window: usize, full: bool) -> Snapshot {
        Snapshot {
            sent: now,
            size,
            delivered: self.delivered,
            delivered_time: self.delivered_time,
            window,
            full,
        }
    }

    fn grow(&self, value: u128) -> u128 {
        if self.startup {
            value * 2
        } else {
            value * 5 / 4
        }
    }

    pub(super) fn ack(&mut self, sample: Snapshot, now: Duration, window: usize) -> usize {
        self.delivered += sample.size;
        self.delivered_time = Some(now);
        self.min_rtt = self.min_rtt.min(now.saturating_sub(sample.sent));

        let Some(first) = self.first_ack else {
            self.first_ack = Some((now, self.delivered));
            return window;
        };
        let (base_time, base_delivered) = sample
            .delivered_time
            .map_or(first, |time| (time, sample.delivered));
        let interval_us = now.saturating_sub(base_time).as_micros();
        if interval_us == 0 {
            // Like C++, same-timestamp/sub-microsecond samples supply no estimate.
            return window;
        }
        let delivered = self.delivered - base_delivered;
        let estimate = if delivered > (MAX_WINDOW as u128) * 2 {
            MAX_WINDOW as u128
        } else {
            // u128 avoids overflow for the product of bytes and microseconds.
            self.grow(delivered * self.min_rtt.as_micros()) / interval_us
        };
        let upper = self.grow(sample.window as u128);
        let lower = if sample.full {
            (sample.window as u128) * 7 / 8
        } else {
            // A delayed app-limited acknowledgement must not undo a newer window.
            window as u128
        };
        let next = estimate
            .min(upper)
            .max(lower)
            .clamp(MIN_WINDOW as u128, MAX_WINDOW as u128) as usize;

        if self.startup && self.round_start.is_none_or(|start| sample.sent >= start) {
            if next as u128 > (self.last_round_window as u128) * 5 / 4 {
                self.plateau_rounds = 0;
            } else {
                self.plateau_rounds += 1;
                if self.plateau_rounds >= 3 {
                    self.startup = false;
                }
            }
            self.round_start = Some(now);
            self.last_round_window = next;
        }
        next
    }
}
