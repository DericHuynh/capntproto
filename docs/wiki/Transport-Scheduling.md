# Local transport scheduling

Each native quiche session has a local scheduling control. TCP sessions do not
provide QUIC packet/datagram scheduling. Obtain
it with `session.scheduling()` before attachment or `handle.scheduling(peer)`
after the RPC route is authenticated and any crossed-dial arbitration has
selected its session. The latter returns `None` for connecting, draining or
terminated routes. No wire message, peer identity or capability authority changes.

```rust
use capntproto::transport::{DatagramPacing, Schedule};
use std::time::Duration;

let scheduling = handle.scheduling(peer).expect("authenticated route");
scheduling.configure(Schedule {
    packet_burst: 8,
    datagrams: Some(DatagramPacing {
        interval: Duration::from_millis(5), // one new datagram credit per 5ms
        burst: 4,                         // retain at most four credits
    }),
})?;
```

`packet_burst` limits the number of packets the driver processes for transmission
before polling other work. It accepts 1–64 and defaults to 16. After exhausting a
burst, a yielding branch keeps the driver runnable even if no socket, application
or timer event is ready. Thus a remaining send backlog does not have to wait for
the transport timeout. A new burst reads the latest policy. The packet count
includes handshake, stream, datagram and control traffic; it does not reorder
frames within a packet or change quiche's congestion control and packet pacing.
Each individual packet still waits for its pacing time and socket writability.
This is a work bound, not a wall-clock latency bound or reserved bandwidth.

`datagrams: None` preserves unrestricted admission from the bounded unreliable
queue. `Some(DatagramPacing)` admits one datagram per credit to quiche, with
intervals from 100 microseconds through one second and bursts of 1–64 datagrams.
Enabling the limiter starts with one full burst. Thereafter it refills one credit
per interval, retaining fractional intervals while below capacity. Idle time
cannot accumulate more than the configured burst. A token is charged even if
quiche subsequently drops/refuses the datagram; the driver does not retry it.
The limit counts datagrams, not payload bytes or on-wire bandwidth. Already
admitted quiche packets retain the transport's existing send/recovery policy.

When credit is exhausted, the driver holds at most one additional datagram and
leaves the rest in the existing 64-entry producer queue. Queue admission and
fragment-batch atomicity are unchanged; a full queue returns `WouldBlock`.
RPC reads/writes, incoming packets, path control and shutdown continue to be
polled while waiting for datagram credit. Admission resumes at the next credit
deadline without requiring another network event. A delayed executor can only
refill up to the burst bound. Realtime payloads still need their receiver-side
deadlines: queueing a datagram neither guarantees delivery nor timely execution.

`configure()` validates and updates synchronously, then wakes the driver.
Reapplying the same datagram policy does not grant new credit or reset its
partial interval, including when only `packet_burst` changes. Switching between
limited policies refills under the old policy, clamps remaining credit to the
new capacity, and starts the new interval at the update time. Removing the
limiter wakes queued work; enabling it again explicitly starts a new full burst.
These are local owner controls, not quotas enforced against a malicious local
application. Invalid settings leave the previous policy intact.

Scheduling handles are weak: retaining one does not retain its session. Driver
termination, including cancellation before its first poll, closes the control;
later operations return `BrokenPipe`. Route replacement starts with a fresh
default policy. An old handle cannot configure the successor. Applications can
configure a connector's returned session before attachment when they want a
policy on every newly authenticated connection. Policy survives CID/path
migration of the same session, which preserves its RPC generation.

Graceful shutdown drops queued/held unreliable work and continues draining the
reliable stream and its existing acknowledgement protocol. It does not wait for
the datagram token bucket to empty. Session closure also discards remaining
datagrams; scheduling adds no delivery guarantee or application-call replay.

`stats()` returns saturating local counts: `packet_attempts` counts completed
packet-send processing (including probes discarded for retired local paths),
`datagram_attempts` counts admissions attempted at quiche, and `burst_yields`
counts selection of the yielding branch after a full packet burst. Other ready
branches may be selected instead and also service other work. These counters
are not acknowledgements or evidence of remote execution. They remain available
only while the driver is alive; callers can retain their own snapshots.

## Verification

```sh
cargo test --locked --lib transport::scheduling::
cargo test --locked --test native_scheduling --test native_shutdown
```

`NativeScheduling.tla` explores **3,915 states**, with **9,135 transition-prefix
replays** through the production credit bucket, policy control, burst guard and
driver retirement. Its bounds are two credits, one refill tick, three admission
attempts, three packet attempts, one policy change/reapplication and one burst
reset. Three negative controls detect minted credit, packet-burst overflow and
admission after closure. This is a safety model; it does not assert fair OS
scheduling, network delivery, hard latency or composed transport refinement.

Clock regressions check fractional refill, long idle periods, capacity changes,
unlimited mode, invalid settings and cancellation before first polling. Live
shared-listener tests exercise both ordinary and arbitrated RPC: a full paced
datagram queue, small calls, a 256 KiB response, CID rotation, validated client
migration, waking an idle credit waiter, graceful shutdown and stale controls
across reconnect. Existing transport, handoff, realtime and shutdown suites cover
the integration with their contracts. Deployment throughput/latency/soak
qualification, adaptive scheduling, per-capability priorities and reserved
bandwidth remain separate work.
