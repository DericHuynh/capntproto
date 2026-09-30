// Copyright (C) 2026, ReProto contributors.
// All rights reserved.
//
// Redistribution and use in source and binary forms, with or without
// modification, are permitted provided that the following conditions are
// met:
//
//     * Redistributions of source code must retain the above copyright notice,
//       this list of conditions and the following disclaimer.
//
//     * Redistributions in binary form must reproduce the above copyright
//       notice, this list of conditions and the following disclaimer in the
//       documentation and/or other materials provided with the distribution.
//
// THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS
// IS" AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO,
// THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR
// PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR
// CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL,
// EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO,
// PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR
// PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF
// LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING
// NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS
// SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

//! Test-only clock and entropy scope for the real Noise transport.
#![forbid(unsafe_code)]

use std::cell::RefCell;
use std::marker::PhantomData;
use std::rc::Rc;
use std::time::Duration;
use std::time::Instant;

// SplitMix64 is deliberately not a cryptographic RNG. Its fixed algorithm and
// byte order make test seeds portable; only cfg(test) builds include this
// module.
struct Random(u64);

impl Random {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        z ^ (z >> 31)
    }
}

struct Environment {
    base: Instant,
    elapsed: Duration,
    entropy: Random,
}

thread_local! {
    static ENV: RefCell<Option<Environment>> = const { RefCell::new(None) };
}

// A scope cannot move to another thread, nest, or survive an unwinding test.
// Both peers run synchronously on the owning thread, as quiche's API permits.
pub(crate) struct Scope(PhantomData<Rc<()>>);

impl Scope {
    pub(crate) fn new(seed: u64) -> Self {
        ENV.with_borrow_mut(|env| {
            assert!(env.is_none(), "nested simulation scope");
            *env = Some(Environment {
                base: Instant::now(),
                elapsed: Duration::ZERO,
                entropy: Random(seed),
            });
        });
        Self(PhantomData)
    }

    pub(crate) fn advance_to(&self, elapsed: Duration) {
        ENV.with_borrow_mut(|env| {
            let env = env.as_mut().unwrap();
            assert!(elapsed >= env.elapsed, "simulation clock went backwards");
            env.base.checked_add(elapsed).unwrap();
            env.elapsed = elapsed;
        });
    }

    fn elapsed(&self) -> Duration {
        ENV.with_borrow(|env| env.as_ref().unwrap().elapsed)
    }

    fn offset(&self, instant: Instant) -> u64 {
        ENV.with_borrow(|env| {
            instant
                .saturating_duration_since(env.as_ref().unwrap().base)
                .as_nanos()
                .try_into()
                .unwrap()
        })
    }
}

impl Drop for Scope {
    fn drop(&mut self) {
        ENV.with_borrow_mut(|env| *env = None);
    }
}

pub(crate) fn now() -> Option<Instant> {
    ENV.with_borrow(|env| env.as_ref().map(|env| env.base + env.elapsed))
}

pub(crate) fn fill_bytes(bytes: &mut [u8]) -> bool {
    ENV.with_borrow_mut(|env| {
        let Some(env) = env else { return false };
        for chunk in bytes.chunks_mut(8) {
            chunk.copy_from_slice(
                &env.entropy.next().to_le_bytes()[..chunk.len()],
            );
        }
        true
    })
}

mod packet;
