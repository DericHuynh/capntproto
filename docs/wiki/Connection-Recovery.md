# Connection recovery and answer setup deadlines

`native_rpc::Network::with_options` configures connection setup, failed-route
recovery and third-party answer setup. The policies operate inside the existing
Tokio `LocalSet`; no background reconnect loop is required.

```rust
use capntproto::native_rpc::{Network, Options, RetryingConnector, RetryPolicy};
use std::rc::Rc;

// `provisioning` is an authorized ProvisioningConnector for permitted peers.
let connector = Rc::new(RetryingConnector::new(
    provisioning,
    RetryPolicy::default(),
)?);
let (network, handle) = Network::with_options(
    local_public_key,
    Some(connector),
    Options {
        recover_failed_routes: true,
        ..Options::default()
    },
)?;
```

## Connection policy

`Options::connect_timeout` bounds the complete setup operation from endpoint
allocation, including time before the driver is polled, provisioning, attempts,
backoff and arbitration. It defaults to ten seconds and
accepts values in `(0, 60s]`. Arbitration also retains its own selection timeout;
the earlier deadline applies. When both the total deadline and retry backoff
become ready after an executor delay, the total deadline wins before another
dial starts.

`RetryingConnector` wraps an existing connector. Defaults allow three attempts,
two seconds per attempt, and exponential backoff starting at 100 ms and capped
at one second. Configuration allows at most eight attempts, sixty seconds per
attempt and ten seconds of backoff. Only `Disconnected`, `Overloaded` and an
attempt timeout permit retry. Invalid identities, malformed tickets and
unconfigured authority fail without retry. Connectors should classify transient
failures explicitly; unknown failures remain terminal. These defaults are bounded
policies, not workload tuning or jittered fleet-wide retry scheduling.

Each attempt calls the underlying connector again. A `ProvisioningConnector`
obtains a fresh single-use reservation; a consumed `DirectoryConnector` reservation
cannot become reusable. Dropping setup cancels its current dial or backoff and
releases its provisioning lease. No detached retry task keeps it alive. The
network shares one pending endpoint among concurrent callers, including across
these attempts. RPC bytes remain buffered until a session is authenticated and,
where configured, arbitration selects it.

With `recover_failed_routes: true`, a later connect or authenticated attach can
replace a failed route even while old clients still retain it. The replacement
gets a new generation and connection identity. Old workers are canceled, old
observers preserve the first failure, and releasing old references cannot remove
the replacement. Live, connecting and draining routes retain the existing
sharing/duplicate rules. Network shutdown prevents new admission.

`RouteObserver::generation()` returns the opaque `RouteGeneration` type. Use
`generation.get()` for numeric diagnostics; compare generations only within the
same network. The allocator never recycles released IDs, accepts the final
nonzero `u64` value, and returns `Overloaded` on subsequent route allocation.
Exhaustion preserves existing routes and starts no additional connector attempt.
Owner identity still prevents an old route's cleanup from removing a replacement.

Applications reacquire a bootstrap or use their reconnecting-capability factory
to obtain new authority. Existing capabilities stay broken. No failed method is
replayed, and a lost reply can still mean an operation committed. Recovery does
not migrate grants, pipelines or object identity between sessions. Failed-route
replacement is opt-in; the default continues to require explicit disconnect.

## Runtime answer setup

The runtime requests a separate `Connection::third_party_answer_timeout()` when
it receives a third-party redirect, an unmatched `ThirdPartyAnswer`, or a self
adoption awaiting authorization. Native supplies `Options::answer_setup_timeout`,
default ten seconds, configurable in `(0, 60s]`. Other networks provide their own
deadline future; the default trait implementation has no timer.

Native active-pipeline migration starts a separate fence budget using this same
setting after authenticated adoption. Join completion releases queued calls on
the direct path; fence failure/expiry releases them on the original route. See
[Discovery and Mobility](Discovery-and-Mobility.md). This does not extend the rendezvous timer
or impose a deadline on the application's running method.

Successful authenticated matching cancels both setup timers before application
method completion. A ready authenticated completion wins over a deadline ready
in the same poll. Expiry releases the rendezvous waiter or registration and the
adopted question's ownership; the latter sends Finish. A late answer cannot
resurrect an expired caller registration. Expiry itself does not abort a healthy
connection. Caller cancellation and system shutdown also cancel pending setup.

Finish must still be followed by Return before a question ID can be reused.
Live and finished-but-unreturned adopted questions share a per-connection limit
of 4,096. Exceeding it aborts that connection. This prevents repeated expiry from
turning the bounded rendezvous waiter table into unbounded retained IDs.

This deadline starts when a setup message arrives. It does not bound an ordinary
call whose relay never sends a redirect, nor a method still running after adoption.
Applications retain control of their method deadlines and retry policy. Timers
require a running executor; elapsed time is not a proof of remote non-execution.

## Verification

```sh
cargo nextest run --locked --lib native_rpc::policy
cargo nextest run --locked --test answer_adoption
cargo nextest run --locked --test native_multiparty
```

`RpcAnswerSetup.tla` explores 47 states and 59 edge-prefix traces. The Rust replay
checks either message order, missing redirects/adoptions, early Return, both
expirations, caller cancellation, Finish counts and waiter cleanup against actual
RPC wire handling. Four mutations violate authentication, non-resurrection or
cleanup invariants. Fair local expiry establishes setup settlement, without
assuming network delivery or application method completion.

`RpcRouteRecovery.tla` explores 113 states and 150 edge-prefix traces through real
Native endpoints. It covers failure/timeout, connect and incoming-attach recovery,
sharing, authentication, old-reference release, shutdown and preserved observer
causes. Four mutations detect reused generations, overwritten causes, premature
publication and old cleanup removing the replacement. Fair local expiry settles
pending setup. This model bounds recovery to two generations and abstracts the
cryptographic handshake as success; it does not prove cryptography or arbitrary
scheduling.

Both new models generate fresh traces directly from TLC graphs in Rust. Virtual
time tests check retry limits, backoff, per-attempt timeout, cancellation and the
overall deadline race. UDP regressions check delivery without replay and stale
generation isolation. An adversarial wire test expires 4,096 unanswered adoptions
and verifies rejection of the next allocation. The full Cargo suite also retains
the prior handoff, adoption, arbitration and provisioning checks.
