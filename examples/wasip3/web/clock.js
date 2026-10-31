async function waitUntil(mark) {
  for (;;) {
    const remaining = mark - monotonicClock.now();
    if (remaining <= 0n) return;
    await new Promise(resolve => setTimeout(resolve, Math.min(Number((remaining + 999_999n) / 1_000_000n), 0x7fffffff)));
  }
}
export const monotonicClock = {
  now: () => BigInt(Math.floor(performance.now() * 1e6)),
  getResolution: () => 1000n,
  waitFor: duration => waitUntil(monotonicClock.now() + duration),
  waitUntil,
};
