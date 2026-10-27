# Failed TLA+ replay tests

[Workflow run and full logs](https://github.com/DericHuynh/capntproto/actions/runs/40441022963) · commit `eecb9aef1eaf0f56d34e409565a6a2d4937a7f4d` · attempt 1

Recorded: **1 failed**, **0 without a terminal result**, 232 passed and 7 skipped.

<details open>
<summary>1. <code>replay_tlc_native_route_traces</code></summary>

<p>capntproto::native_multiparty</p>
<pre>thread &#x27;replay_tlc_native_route_traces&#x27; (44636) panicked at tests&#x2F;native_multiparty.rs:693:9:
called `Result::unwrap()` on an `Err` value: Kind(TimedOut)
stack backtrace:
   0: __rustc::rust_begin_unwind
             at &#x2F;rustc&#x2F;17fd5b8a37b6667b6cc137f3cc35f09759768a3b&#x2F;library&#x2F;std&#x2F;src&#x2F;panicking.rs:676:5
   1: core::panicking::panic_fmt
             at &#x2F;rustc&#x2F;17fd5b8a37b6667b6cc137f3cc35f09759768a3b&#x2F;library&#x2F;core&#x2F;src&#x2F;panicking.rs:80:14
   2: core::result::unwrap_failed
             at &#x2F;rustc&#x2F;17fd5b8a37b6667b6cc137f3cc35f09759768a3b&#x2F;library&#x2F;core&#x2F;src&#x2F;result.rs:1870:5
   3: &lt;core::result::Result&lt;capntproto::transport::AuthenticatedSession, core::io::error::Error&gt;&gt;::unwrap
             at &#x2F;home&#x2F;runner&#x2F;.rustup&#x2F;toolchains&#x2F;nightly-2026-08-29-x86_64-unknown-linux-gnu&#x2F;lib&#x2F;rustlib&#x2F;src&#x2F;rust&#x2F;library&#x2F;core&#x2F;src&#x2F;result.rs:1232:23
   4: native_multiparty::sessions::{closure#0}
             at .&#x2F;tests&#x2F;native_multiparty.rs:693:9
   5: native_multiparty::replay_route::{closure#0}
             at .&#x2F;tests&#x2F;native_multiparty.rs:1238:57
   6: &lt;tokio::time::timeout::Timeout&lt;native_multiparty::replay_route::{closure#0}&gt; as core::future::future::Future&gt;::poll
             at &#x2F;home&#x2F;runner&#x2F;.cargo&#x2F;registry&#x2F;src&#x2F;index.crates.io-1949cf8c6b5b557f&#x2F;tokio-1.53.3&#x2F;src&#x2F;time&#x2F;timeout.rs:217:42
   7: native_multiparty::replay_tlc_native_route_traces::{closure#0}::{closure#0}
             at .&#x2F;tests&#x2F;native_multiparty.rs:1212:22
   8: &lt;tokio::task::local::RunUntil&lt;native_multiparty::replay_tlc_native_route_traces::{closure#0}::{closure#0}&gt; as core::future::future::Future&gt;::poll::{closure#0}
             at &#x2F;home&#x2F;runner&#x2F;.cargo&#x2F;registry&#x2F;src&#x2F;index.crates.io-1949cf8c6b5b557f&#x2F;tokio-1.53.3&#x2F;src&#x2F;task&#x2F;local.rs:1073:44
   9: &lt;tokio::task::local::LocalSet&gt;::with::&lt;core::task::poll::Poll&lt;()&gt;, &lt;tokio::task::local::RunUntil&lt;native_multiparty::replay_tlc_native_route_traces::{closure#0}::{closure#0}&gt; as core::future::future::Future&gt;::poll::{closure#0}&gt;::{closure#0}
             at &#x2F;home&#x2F;runner&#x2F;.cargo&#x2F;registry&#x2F;src&#x2F;index.crates.io-1949cf8c6b5b557f&#x2F;tokio-1.53.3&#x2F;src&#x2F;task&#x2F;local.rs:824:13
  10: &lt;std::thread::local::LocalKey&lt;tokio::task::local::LocalData&gt;&gt;::try_with::&lt;&lt;tokio::task::local::LocalSet&gt;::with&lt;core::task::poll::Poll&lt;()&gt;, &lt;tokio::task::local::RunUntil&lt;native_multiparty::replay_tlc_native_route_traces::{closure#0}::{closure#0}&gt; as core::future::future::Future&gt;::poll::{closure#0}&gt;::{closure#0}, core::task::poll::Poll&lt;()&gt;&gt;
             at &#x2F;home&#x2F;runner&#x2F;.rustup&#x2F;toolchains&#x2F;nightly-2026-08-29-x86_64-unknown-linux-gnu&#x2F;lib&#x2F;rustlib&#x2F;src&#x2F;rust&#x2F;library&#x2F;std&#x2F;src&#x2F;thread&#x2F;local.rs:463:12
  11: &lt;std::thread::local::LocalKey&lt;tokio::task::local::LocalData&gt;&gt;::with::&lt;&lt;tokio::task::local::LocalSet&gt;::with&lt;core::task::poll::Poll&lt;()&gt;, &lt;tokio::task::local::RunUntil&lt;native_multiparty::replay_tlc_native_route_traces::{closure#0}::{closure#0}&gt; as core::future::future::Future&gt;::poll::{closure#0}&gt;::{closure#0}, core::task::poll::Poll&lt;()&gt;&gt;
             at &#x2F;home&#x2F;runner&#x2F;.rustup&#x2F;toolchains&#x2F;nightly-2026-08-29-x86_64-unknown-linux-gnu&#x2F;lib&#x2F;rustlib&#x2F;src&#x2F;rust&#x2F;library&#x2F;std&#x2F;src&#x2F;thread&#x2F;local.rs:427:20
  12: &lt;tokio::task::local::LocalSet&gt;::with::&lt;core::task::poll::Poll&lt;()&gt;, &lt;tokio::task::local::RunUntil&lt;native_multiparty::replay_tlc_native_route_traces::{closure#0}::{closure#0}&gt; as core::future::future::Future&gt;::poll::{closure#0}&gt;
             at &#x2F;home&#x2F;runner&#x2F;.cargo&#x2F;registry&#x2F;src&#x2F;index.crates.io-1949cf8c6b5b557f&#x2F;tokio-1.53.3&#x2F;src&#x2F;task&#x2F;local.rs:822:17
  13: &lt;tokio::task::local::RunUntil&lt;native_multiparty::replay_tlc_native_route_traces::{closure#0}::{closure#0}&gt; as core::future::future::Future&gt;::poll
             at &#x2F;home&#x2F;runner&#x2F;.cargo&#x2F;registry&#x2F;src&#x2F;index.crates.io-1949cf8c6b5b557f&#x2F;tokio-1.53.3&#x2F;src&#x2F;task&#x2F;local.rs:1063:22
  14: &lt;tokio::task::local::LocalSet&gt;::run_until::&lt;native_multiparty::replay_tlc_native_route_traces::{closure#0}::{closure#0}&gt;::{closure#0}
             at &#x2F;home&#x2F;runner&#x2F;.cargo&#x2F;registry&#x2F;src&#x2F;index.crates.io-1949cf8c6b5b557f&#x2F;tokio-1.53.3&#x2F;src&#x2F;task&#x2F;local.rs:720:19
  15: native_multiparty::replay_tlc_native_route_traces::{closure#0}
             at .&#x2F;tests&#x2F;native_multiparty.rs:1216:10
  16: &lt;core::pin::Pin&lt;&amp;mut dyn core::fu</pre>

Diagnostic excerpt truncated; full output is in the workflow artifact.

</details>

The workspace command failed or was incomplete. Build/infrastructure errors and tests that never finished are not invented as named failures.
