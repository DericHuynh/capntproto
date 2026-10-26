# Failed workspace tests

[Workflow run and full logs](https://github.com/DericHuynh/capntproto/actions/runs/40321608784) · commit `16b04ee9f981663e2e8213943be97b434b8f0e4d` · attempt 1

Recorded: **5 failed**, **0 without a terminal result**, 2496 passed and 20 skipped.

<details open>
<summary>1. <code>portable_adoption_exposes_unused_pipeline_before_return_and_preserves_used_route</code></summary>

<p>capntproto::answer_adoption</p>
<pre>thread &#x27;portable_adoption_exposes_unused_pipeline_before_return_and_preserves_used_route&#x27; (32901) panicked at tests&#x2F;answer_adoption.rs:1247:21:
assertion `left == right` failed: post-adoption pipeline call was not published; used=false
  left: 0
 right: 1
stack backtrace:
   0: __rustc::rust_begin_unwind
   1: core::panicking::panic_fmt
   2: core::panicking::assert_failed_inner
   3: core::panicking::assert_failed::&lt;usize, usize&gt;
   4: answer_adoption::portable_adoption_exposes_unused_pipeline_before_return_and_preserves_used_route::{closure#0}::{closure#0}::{closure#0}
   5: &lt;tokio::time::timeout::Timeout&lt;answer_adoption::portable_adoption_exposes_unused_pipeline_before_return_and_preserves_used_route::{closure#0}::{closure#0}::{closure#0}&gt; as core::future::future::Future&gt;::poll
   6: answer_adoption::portable_adoption_exposes_unused_pipeline_before_return_and_preserves_used_route::{closure#0}::{closure#0}
   7: &lt;tokio::task::local::RunUntil&lt;answer_adoption::portable_adoption_exposes_unused_pipeline_before_return_and_preserves_used_route::{closure#0}::{closure#0}&gt; as core::future::future::Future&gt;::poll::{closure#0}
   8: &lt;tokio::task::local::LocalSet&gt;::with::&lt;core::task::poll::Poll&lt;()&gt;, &lt;tokio::task::local::RunUntil&lt;answer_adoption::portable_adoption_exposes_unused_pipeline_before_return_and_preserves_used_route::{closure#0}::{closure#0}&gt; as core::future::future::Future&gt;::poll::{closure#0}&gt;::{closure#0}
   9: &lt;tokio::task::local::LocalSet&gt;::run_until::&lt;answer_adoption::portable_adoption_exposes_unused_pipeline_before_return_and_preserves_used_route::{closure#0}::{closure#0}&gt;::{closure#0}
  10: answer_adoption::portable_adoption_exposes_unused_pipeline_before_return_and_preserves_used_route::{closure#0}
  11: tokio::task::coop::budget::&lt;core::task::poll::Poll&lt;()&gt;, &lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::block_on&lt;core::pin::Pin&lt;&amp;mut core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Output = ()&gt;&gt;&gt;&gt;::{closure#0}::{closure#0}::{closure#0}&gt;
  12: &lt;tokio::runtime::scheduler::current_thread::Context&gt;::enter::&lt;core::task::poll::Poll&lt;()&gt;, &lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::block_on&lt;core::pin::Pin&lt;&amp;mut core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Output = ()&gt;&gt;&gt;&gt;::{closure#0}::{closure#0}&gt;
  13: &lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::block_on::&lt;core::pin::Pin&lt;&amp;mut core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Output = ()&gt;&gt;&gt;&gt;::{closure#0}
  14: &lt;tokio::runtime::context::scoped::Scoped&lt;tokio::runtime::scheduler::Context&gt;&gt;::set::&lt;&lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::enter&lt;&lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::block_on&lt;core::pin::Pin&lt;&amp;mut core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Output = ()&gt;&gt;&gt;&gt;::{closure#0}, core::option::Option&lt;()&gt;&gt;::{closure#0}, (alloc::boxed::Box&lt;tokio::runtime::scheduler::current_thread::Core&gt;, core::option::Option&lt;()&gt;)&gt;
  15: &lt;std::thread::local::LocalKey&lt;tokio::runtime::context::Context&gt;&gt;::with::&lt;tokio::runtime::context::set_scheduler&lt;(alloc::boxed::Box&lt;tokio::runtime::scheduler::current_thread::Core&gt;, core::option::Option&lt;()&gt;), &lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::enter&lt;&lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::block_on&lt;core::pin::Pin&lt;&amp;mut core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Output = ()&gt;&gt;&gt;&gt;::{closure#0}, core::option::Option&lt;()&gt;&gt;::{closure#0}&gt;::{closure#0}, (alloc::boxed::Box&lt;tokio::runtime::scheduler::current_thread::Core&gt;, core::option::Option&lt;()&gt;)&gt;
  16: &lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::enter::&lt;&lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::block_on&lt;core::pin::Pin&lt;&amp;mut core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Output = ()&gt;&gt;&gt;&gt;::{closure#0}, core::option::Option&lt;()&gt;&gt;
  17: &lt;tokio::runtime::scheduler::current_thread::CurrentThread&gt;::block_on::&lt;core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Output = ()&gt;&gt;&gt;::{closure#0}
  18: tokio::runtime::context::runtime::enter_runtime::&lt;&lt;tokio::runtime::scheduler::current_thread::CurrentThread&gt;::block_on&lt;core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Out</pre>

Diagnostic excerpt truncated; full output is in the workflow artifact.

</details>

<details open>
<summary>2. <code>portable_tail_finish_before_child_completion_reclaims_both_interests</code></summary>

<p>capntproto::answer_adoption</p>
<pre>thread &#x27;portable_tail_finish_before_child_completion_reclaims_both_interests&#x27; (32907) panicked at tests&#x2F;answer_adoption.rs:2577:17:
assertion `left == right` failed: both canceled answers retained application work
  left: 0
 right: 1
stack backtrace:
   0: __rustc::rust_begin_unwind
   1: core::panicking::panic_fmt
   2: core::panicking::assert_failed_inner
   3: core::panicking::assert_failed::&lt;u32, u32&gt;
   4: answer_adoption::portable_tail_finish_before_child_completion_reclaims_both_interests::{closure#0}::{closure#0}::{closure#0}
   5: &lt;tokio::time::timeout::Timeout&lt;answer_adoption::portable_tail_finish_before_child_completion_reclaims_both_interests::{closure#0}::{closure#0}::{closure#0}&gt; as core::future::future::Future&gt;::poll
   6: answer_adoption::portable_tail_finish_before_child_completion_reclaims_both_interests::{closure#0}::{closure#0}
   7: &lt;tokio::task::local::RunUntil&lt;answer_adoption::portable_tail_finish_before_child_completion_reclaims_both_interests::{closure#0}::{closure#0}&gt; as core::future::future::Future&gt;::poll::{closure#0}
   8: &lt;tokio::task::local::LocalSet&gt;::with::&lt;core::task::poll::Poll&lt;()&gt;, &lt;tokio::task::local::RunUntil&lt;answer_adoption::portable_tail_finish_before_child_completion_reclaims_both_interests::{closure#0}::{closure#0}&gt; as core::future::future::Future&gt;::poll::{closure#0}&gt;::{closure#0}
   9: &lt;tokio::task::local::LocalSet&gt;::run_until::&lt;answer_adoption::portable_tail_finish_before_child_completion_reclaims_both_interests::{closure#0}::{closure#0}&gt;::{closure#0}
  10: answer_adoption::portable_tail_finish_before_child_completion_reclaims_both_interests::{closure#0}
  11: tokio::task::coop::budget::&lt;core::task::poll::Poll&lt;()&gt;, &lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::block_on&lt;core::pin::Pin&lt;&amp;mut core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Output = ()&gt;&gt;&gt;&gt;::{closure#0}::{closure#0}::{closure#0}&gt;
  12: &lt;tokio::runtime::scheduler::current_thread::Context&gt;::enter::&lt;core::task::poll::Poll&lt;()&gt;, &lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::block_on&lt;core::pin::Pin&lt;&amp;mut core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Output = ()&gt;&gt;&gt;&gt;::{closure#0}::{closure#0}&gt;
  13: &lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::block_on::&lt;core::pin::Pin&lt;&amp;mut core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Output = ()&gt;&gt;&gt;&gt;::{closure#0}
  14: &lt;tokio::runtime::context::scoped::Scoped&lt;tokio::runtime::scheduler::Context&gt;&gt;::set::&lt;&lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::enter&lt;&lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::block_on&lt;core::pin::Pin&lt;&amp;mut core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Output = ()&gt;&gt;&gt;&gt;::{closure#0}, core::option::Option&lt;()&gt;&gt;::{closure#0}, (alloc::boxed::Box&lt;tokio::runtime::scheduler::current_thread::Core&gt;, core::option::Option&lt;()&gt;)&gt;
  15: &lt;std::thread::local::LocalKey&lt;tokio::runtime::context::Context&gt;&gt;::with::&lt;tokio::runtime::context::set_scheduler&lt;(alloc::boxed::Box&lt;tokio::runtime::scheduler::current_thread::Core&gt;, core::option::Option&lt;()&gt;), &lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::enter&lt;&lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::block_on&lt;core::pin::Pin&lt;&amp;mut core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Output = ()&gt;&gt;&gt;&gt;::{closure#0}, core::option::Option&lt;()&gt;&gt;::{closure#0}&gt;::{closure#0}, (alloc::boxed::Box&lt;tokio::runtime::scheduler::current_thread::Core&gt;, core::option::Option&lt;()&gt;)&gt;
  16: &lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::enter::&lt;&lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::block_on&lt;core::pin::Pin&lt;&amp;mut core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Output = ()&gt;&gt;&gt;&gt;::{closure#0}, core::option::Option&lt;()&gt;&gt;
  17: &lt;tokio::runtime::scheduler::current_thread::CurrentThread&gt;::block_on::&lt;core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Output = ()&gt;&gt;&gt;::{closure#0}
  18: tokio::runtime::context::runtime::enter_runtime::&lt;&lt;tokio::runtime::scheduler::current_thread::CurrentThread&gt;::block_on&lt;core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Output = ()&gt;&gt;&gt;::{closure#0}, ()&gt;
  19: &lt;tokio::runtime::runtime::Runtime&gt;::block_on_inner::&lt;core::pin::Pin&lt;&amp;mut d</pre>

Diagnostic excerpt truncated; full output is in the workflow artifact.

</details>

<details open>
<summary>3. <code>portable_third_party_pipeline_transfer_resumes_after_idle_at_each_poll_budget</code></summary>

<p>capntproto::answer_adoption</p>
<pre>thread &#x27;portable_third_party_pipeline_transfer_resumes_after_idle_at_each_poll_budget&#x27; (32911) panicked at tests&#x2F;answer_adoption.rs:176:37:
transfer stalled with poll budget 7
stack backtrace:
   0: __rustc::rust_begin_unwind
   1: core::panicking::panic_fmt
   2: answer_adoption::portable_third_party_pipeline_transfer_resumes_after_idle_at_each_poll_budget::{closure#0}::{closure#0}::{closure#1}
   3: answer_adoption::portable_third_party_pipeline_transfer_resumes_after_idle_at_each_poll_budget::{closure#0}::{closure#0}
   4: &lt;tokio::task::local::RunUntil&lt;answer_adoption::portable_third_party_pipeline_transfer_resumes_after_idle_at_each_poll_budget::{closure#0}::{closure#0}&gt; as core::future::future::Future&gt;::poll::{closure#0}
   5: &lt;tokio::task::local::LocalSet&gt;::with::&lt;core::task::poll::Poll&lt;()&gt;, &lt;tokio::task::local::RunUntil&lt;answer_adoption::portable_third_party_pipeline_transfer_resumes_after_idle_at_each_poll_budget::{closure#0}::{closure#0}&gt; as core::future::future::Future&gt;::poll::{closure#0}&gt;::{closure#0}
   6: &lt;tokio::task::local::LocalSet&gt;::run_until::&lt;answer_adoption::portable_third_party_pipeline_transfer_resumes_after_idle_at_each_poll_budget::{closure#0}::{closure#0}&gt;::{closure#0}
   7: answer_adoption::portable_third_party_pipeline_transfer_resumes_after_idle_at_each_poll_budget::{closure#0}
   8: tokio::task::coop::budget::&lt;core::task::poll::Poll&lt;()&gt;, &lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::block_on&lt;core::pin::Pin&lt;&amp;mut core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Output = ()&gt;&gt;&gt;&gt;::{closure#0}::{closure#0}::{closure#0}&gt;
   9: &lt;tokio::runtime::scheduler::current_thread::Context&gt;::enter::&lt;core::task::poll::Poll&lt;()&gt;, &lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::block_on&lt;core::pin::Pin&lt;&amp;mut core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Output = ()&gt;&gt;&gt;&gt;::{closure#0}::{closure#0}&gt;
  10: &lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::block_on::&lt;core::pin::Pin&lt;&amp;mut core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Output = ()&gt;&gt;&gt;&gt;::{closure#0}
  11: &lt;tokio::runtime::context::scoped::Scoped&lt;tokio::runtime::scheduler::Context&gt;&gt;::set::&lt;&lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::enter&lt;&lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::block_on&lt;core::pin::Pin&lt;&amp;mut core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Output = ()&gt;&gt;&gt;&gt;::{closure#0}, core::option::Option&lt;()&gt;&gt;::{closure#0}, (alloc::boxed::Box&lt;tokio::runtime::scheduler::current_thread::Core&gt;, core::option::Option&lt;()&gt;)&gt;
  12: &lt;std::thread::local::LocalKey&lt;tokio::runtime::context::Context&gt;&gt;::with::&lt;tokio::runtime::context::set_scheduler&lt;(alloc::boxed::Box&lt;tokio::runtime::scheduler::current_thread::Core&gt;, core::option::Option&lt;()&gt;), &lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::enter&lt;&lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::block_on&lt;core::pin::Pin&lt;&amp;mut core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Output = ()&gt;&gt;&gt;&gt;::{closure#0}, core::option::Option&lt;()&gt;&gt;::{closure#0}&gt;::{closure#0}, (alloc::boxed::Box&lt;tokio::runtime::scheduler::current_thread::Core&gt;, core::option::Option&lt;()&gt;)&gt;
  13: &lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::enter::&lt;&lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::block_on&lt;core::pin::Pin&lt;&amp;mut core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Output = ()&gt;&gt;&gt;&gt;::{closure#0}, core::option::Option&lt;()&gt;&gt;
  14: &lt;tokio::runtime::scheduler::current_thread::CurrentThread&gt;::block_on::&lt;core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Output = ()&gt;&gt;&gt;::{closure#0}
  15: tokio::runtime::context::runtime::enter_runtime::&lt;&lt;tokio::runtime::scheduler::current_thread::CurrentThread&gt;::block_on&lt;core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Output = ()&gt;&gt;&gt;::{closure#0}, ()&gt;
  16: &lt;tokio::runtime::runtime::Runtime&gt;::block_on_inner::&lt;core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Output = ()&gt;&gt;&gt;
  17: answer_adoption::portable_third_party_pipeline_transfer_resumes_after_idle_at_each_poll_budget
  18: &lt;answer_adoption::portable_third_party_pipeline_transfer_resumes_after_idle_at_each_poll_budget::{closure#0} as core::ops::function::FnOnce&lt;()&gt;&gt;</pre>

Diagnostic excerpt truncated; full output is in the workflow artifact.

</details>

<details open>
<summary>4. <code>third_party_tail_returns_over_authenticated_native</code></summary>

<p>capntproto::answer_adoption</p>
<pre>thread &#x27;third_party_tail_returns_over_authenticated_native&#x27; (32923) panicked at tests&#x2F;answer_adoption.rs:1479:37:
portable=true: stalled at resolve early pipeline
stack backtrace:
   0: __rustc::rust_begin_unwind
   1: core::panicking::panic_fmt
   2: answer_adoption::third_party_tail_returns_over_authenticated_native::{closure#0}::{closure#0}::{closure#1}
   3: answer_adoption::third_party_tail_returns_over_authenticated_native::{closure#0}::{closure#0}
   4: &lt;tokio::task::local::RunUntil&lt;answer_adoption::third_party_tail_returns_over_authenticated_native::{closure#0}::{closure#0}&gt; as core::future::future::Future&gt;::poll::{closure#0}
   5: &lt;tokio::task::local::LocalSet&gt;::with::&lt;core::task::poll::Poll&lt;()&gt;, &lt;tokio::task::local::RunUntil&lt;answer_adoption::third_party_tail_returns_over_authenticated_native::{closure#0}::{closure#0}&gt; as core::future::future::Future&gt;::poll::{closure#0}&gt;::{closure#0}
   6: &lt;tokio::task::local::LocalSet&gt;::run_until::&lt;answer_adoption::third_party_tail_returns_over_authenticated_native::{closure#0}::{closure#0}&gt;::{closure#0}
   7: answer_adoption::third_party_tail_returns_over_authenticated_native::{closure#0}
   8: tokio::task::coop::budget::&lt;core::task::poll::Poll&lt;()&gt;, &lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::block_on&lt;core::pin::Pin&lt;&amp;mut core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Output = ()&gt;&gt;&gt;&gt;::{closure#0}::{closure#0}::{closure#0}&gt;
   9: &lt;tokio::runtime::scheduler::current_thread::Context&gt;::enter::&lt;core::task::poll::Poll&lt;()&gt;, &lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::block_on&lt;core::pin::Pin&lt;&amp;mut core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Output = ()&gt;&gt;&gt;&gt;::{closure#0}::{closure#0}&gt;
  10: &lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::block_on::&lt;core::pin::Pin&lt;&amp;mut core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Output = ()&gt;&gt;&gt;&gt;::{closure#0}
  11: &lt;tokio::runtime::context::scoped::Scoped&lt;tokio::runtime::scheduler::Context&gt;&gt;::set::&lt;&lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::enter&lt;&lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::block_on&lt;core::pin::Pin&lt;&amp;mut core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Output = ()&gt;&gt;&gt;&gt;::{closure#0}, core::option::Option&lt;()&gt;&gt;::{closure#0}, (alloc::boxed::Box&lt;tokio::runtime::scheduler::current_thread::Core&gt;, core::option::Option&lt;()&gt;)&gt;
  12: &lt;std::thread::local::LocalKey&lt;tokio::runtime::context::Context&gt;&gt;::with::&lt;tokio::runtime::context::set_scheduler&lt;(alloc::boxed::Box&lt;tokio::runtime::scheduler::current_thread::Core&gt;, core::option::Option&lt;()&gt;), &lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::enter&lt;&lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::block_on&lt;core::pin::Pin&lt;&amp;mut core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Output = ()&gt;&gt;&gt;&gt;::{closure#0}, core::option::Option&lt;()&gt;&gt;::{closure#0}&gt;::{closure#0}, (alloc::boxed::Box&lt;tokio::runtime::scheduler::current_thread::Core&gt;, core::option::Option&lt;()&gt;)&gt;
  13: &lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::enter::&lt;&lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::block_on&lt;core::pin::Pin&lt;&amp;mut core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Output = ()&gt;&gt;&gt;&gt;::{closure#0}, core::option::Option&lt;()&gt;&gt;
  14: &lt;tokio::runtime::scheduler::current_thread::CurrentThread&gt;::block_on::&lt;core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Output = ()&gt;&gt;&gt;::{closure#0}
  15: tokio::runtime::context::runtime::enter_runtime::&lt;&lt;tokio::runtime::scheduler::current_thread::CurrentThread&gt;::block_on&lt;core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Output = ()&gt;&gt;&gt;::{closure#0}, ()&gt;
  16: &lt;tokio::runtime::runtime::Runtime&gt;::block_on_inner::&lt;core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Output = ()&gt;&gt;&gt;
  17: answer_adoption::third_party_tail_returns_over_authenticated_native
  18: &lt;answer_adoption::third_party_tail_returns_over_authenticated_native::{closure#0} as core::ops::function::FnOnce&lt;()&gt;&gt;::call_once
note: Some details are omitted, run with `RUST_BACKTRACE=full` for a verbose backtrace.

running 1 test
test third_party_tail_returns_over_authenticated_native ... FAILED

failures:

failures:
    third_party_tail_ret</pre>

Diagnostic excerpt truncated; full output is in the workflow artifact.

</details>

<details open>
<summary>5. <code>independent_clients_share_a_real_service_and_pipeline_deep_access</code></summary>

<p>capntproto::service_examples</p>
<pre>thread &#x27;independent_clients_share_a_real_service_and_pipeline_deep_access&#x27; (39342) panicked at tests&#x2F;service_examples.rs:163:61:
called `Result::unwrap()` on an `Err` value: &quot;timed out: cd \&quot;&#x2F;home&#x2F;runner&#x2F;work&#x2F;capntproto&#x2F;capntproto\&quot; &amp;&amp; env -u CAPNTPROTO_COVERAGE_CHILDREN_NATIVE -u CARGO_ENCODED_RUSTFLAGS -u CARGO_TARGET_DIR -u NEXTEST_PROFILE -u RUSTUP_TOOLCHAIN LLVM_PROFILE_FILE=\&quot;&#x2F;home&#x2F;runner&#x2F;work&#x2F;capntproto&#x2F;capntproto&#x2F;target&#x2F;quality-build&#x2F;0c9a8cbce3b093165104c44cbb4a0ea08d704d42126e67efc8e81534cce5204f&#x2F;profiles-bTzFVT&#x2F;%p-%m.profraw\&quot; RUSTDOCFLAGS=\&quot;-C instrument-coverage -C link-dead-code -C opt-level=1 -C debug-assertions=yes -C overflow-checks=yes -Z coverage-options=branch\&quot; RUSTFLAGS=\&quot;-C instrument-coverage -C link-dead-code -C opt-level=1 -C debug-assertions=yes -C overflow-checks=yes -Z coverage-options=branch\&quot; \&quot;cargo\&quot; \&quot;+nightly-2026-03-05\&quot; \&quot;build\&quot; \&quot;--locked\&quot; \&quot;--manifest-path\&quot; \&quot;examples&#x2F;kv&#x2F;Cargo.toml\&quot; \&quot;--bins\&quot; \&quot;--target-dir\&quot; \&quot;&#x2F;home&#x2F;runner&#x2F;work&#x2F;capntproto&#x2F;capntproto&#x2F;target&#x2F;quality-build&#x2F;0c9a8cbce3b093165104c44cbb4a0ea08d704d42126e67efc8e81534cce5204f&#x2F;service-examples\&quot; \&quot;--ignore-rust-version\&quot; \&quot;--features\&quot; \&quot;quic\&quot;; inspect &#x2F;tmp&#x2F;.tmpAyraba&#x2F;build.log&quot;
stack backtrace:
   0: __rustc::rust_begin_unwind
   1: core::panicking::panic_fmt
   2: core::result::unwrap_failed
   3: service_examples::independent_clients_share_a_real_service_and_pipeline_deep_access
   4: &lt;service_examples::independent_clients_share_a_real_service_and_pipeline_deep_access::{closure#0} as core::ops::function::FnOnce&lt;()&gt;&gt;::call_once
note: Some details are omitted, run with `RUST_BACKTRACE=full` for a verbose backtrace.

running 1 test
test independent_clients_share_a_real_service_and_pipeline_deep_access has been running for over 60 seconds
test independent_clients_share_a_real_service_and_pipeline_deep_access ... FAILED

failures:

failures:
    independent_clients_share_a_real_service_and_pipeline_deep_access

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 1 filtered out; finished in 600.45s



thread &#x27;independent_clients_share_a_real_service_and_pipeline_deep_access&#x27; (39342) panicked at tests&#x2F;service_examples.rs:163:61:
called `Result::unwrap()` on an `Err` value: &quot;timed out: cd \&quot;&#x2F;home&#x2F;runner&#x2F;work&#x2F;capntproto&#x2F;capntproto\&quot; &amp;&amp; env -u CAPNTPROTO_COVERAGE_CHILDREN_NATIVE -u CARGO_ENCODED_RUSTFLAGS -u CARGO_TARGET_DIR -u NEXTEST_PROFILE -u RUSTUP_TOOLCHAIN LLVM_PROFILE_FILE=\&quot;&#x2F;home&#x2F;runner&#x2F;work&#x2F;capntproto&#x2F;capntproto&#x2F;target&#x2F;quality-build&#x2F;0c9a8cbce3b093165104c44cbb4a0ea08d704d42126e67efc8e81534cce5204f&#x2F;profiles-bTzFVT&#x2F;%p-%m.profraw\&quot; RUSTDOCFLAGS=\&quot;-C instrument-coverage -C link-dead-code -C opt-level=1 -C debug-assertions=yes -C overflow-checks=yes -Z coverage-options=branch\&quot; RUSTFLAGS=\&quot;-C instrument-coverage -C link-dead-code -C opt-level=1 -C debug-assertions=yes -C overflow-checks=yes -Z coverage-options=branch\&quot; \&quot;cargo\&quot; \&quot;+nightly-2026-03-05\&quot; \&quot;build\&quot; \&quot;--locked\&quot; \&quot;--manifest-path\&quot; \&quot;examples&#x2F;kv&#x2F;Cargo.toml\&quot; \&quot;--bins\&quot; \&quot;--target-dir\&quot; \&quot;&#x2F;home&#x2F;runner&#x2F;work&#x2F;capntproto&#x2F;capntproto&#x2F;target&#x2F;quality-build&#x2F;0c9a8cbce3b093165104c44cbb4a0ea08d704d42126e67efc8e81534cce5204f&#x2F;service-examples\&quot; \&quot;--ignore-rust-version\&quot; \&quot;--features\&quot; \&quot;quic\&quot;; inspect &#x2F;tmp&#x2F;.tmpAyraba&#x2F;build.log&quot;
stack backtrace:
   0: __rustc::rust_begin_unwind
   1: core::panicking::panic_fmt
   2: core::result::unwrap_failed
   3: service_examples::independent_clients_share_a_real_service_and_pipeline_deep_access
   4: &lt;service_examples::independent_clients_share_a_real_service_and_pipeline_deep_access::{closure#0} as core::ops::function::FnOnce&lt;()&gt;&gt;::call_once
note: Some details are omitted, run with `RUST_BACKTRACE=full` for a verbose backtrace.
</pre>

</details>

The workspace command failed or was incomplete. Build/infrastructure errors and tests that never finished are not invented as named failures.
