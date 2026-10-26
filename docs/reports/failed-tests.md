# Failed workspace tests

[Workflow run and full logs](https://github.com/DericHuynh/capntproto/actions/runs/40255382264) · commit `b81dd7cf4e4e6ee9100d4aa8215975f2baf9b7c7` · attempt 1

Recorded: **9 failed**, **0 without a terminal result**, 2473 passed and 20 skipped.

<details open>
<summary>1. <code>rpc::io::tests::framing_keeps_vectored_writes_through_pending_and_partial_progress</code></summary>

<p>capntproto</p>
<pre>thread &#x27;rpc::io::tests::framing_keeps_vectored_writes_through_pending_and_partial_progress&#x27; (32091) panicked at src&#x2F;rpc&#x2F;io.rs:163:13:
vectored writes must reach the transport intact
stack backtrace:
   0: __rustc::rust_begin_unwind
   1: core::panicking::panic_fmt
   2: &lt;capntproto::rpc::io::tests::Writer as tokio::io::async_write::AsyncWrite&gt;::poll_write
   3: &lt;futures_util::io::write_all::WriteAll&lt;&amp;mut capntproto::rpc::io::TokioIo&lt;capntproto::rpc::io::tests::Writer&gt;&gt; as core::future::future::Future&gt;::poll
   4: capnp_futures::serialize::write_framed::&lt;&amp;mut capntproto::rpc::io::TokioIo&lt;capntproto::rpc::io::tests::Writer&gt;&gt;::{closure#0}
   5: capnp_futures::serialize::write_buffers::&lt;&amp;mut capntproto::rpc::io::TokioIo&lt;capntproto::rpc::io::tests::Writer&gt;&gt;::{closure#0}
   6: capnp_futures::serialize::write_message_refs::&lt;&amp;mut capntproto::rpc::io::TokioIo&lt;capntproto::rpc::io::tests::Writer&gt;, capnp::message::Builder&lt;capnp::message::HeapAllocator&gt;, core::slice::iter::Iter&lt;capnp::message::Builder&lt;capnp::message::HeapAllocator&gt;&gt;&gt;::{closure#0}
   7: capnp_futures::serialize::write_messages::&lt;&amp;mut capntproto::rpc::io::TokioIo&lt;capntproto::rpc::io::tests::Writer&gt;, capnp::message::Builder&lt;capnp::message::HeapAllocator&gt;&gt;::{closure#0}
   8: capntproto::rpc::io::tests::framing_keeps_vectored_writes_through_pending_and_partial_progress::{closure#0}
   9: futures_executor::local_pool::run_executor::&lt;(), futures_executor::local_pool::block_on&lt;capntproto::rpc::io::tests::framing_keeps_vectored_writes_through_pending_and_partial_progress::{closure#0}&gt;::{closure#0}&gt;::{closure#0}
  10: futures_executor::local_pool::block_on::&lt;capntproto::rpc::io::tests::framing_keeps_vectored_writes_through_pending_and_partial_progress::{closure#0}&gt;
  11: capntproto::rpc::io::tests::framing_keeps_vectored_writes_through_pending_and_partial_progress
  12: &lt;capntproto::rpc::io::tests::framing_keeps_vectored_writes_through_pending_and_partial_progress::{closure#0} as core::ops::function::FnOnce&lt;()&gt;&gt;::call_once
note: Some details are omitted, run with `RUST_BACKTRACE=full` for a verbose backtrace.

running 1 test
test rpc::io::tests::framing_keeps_vectored_writes_through_pending_and_partial_progress ... FAILED

failures:

failures:
    rpc::io::tests::framing_keeps_vectored_writes_through_pending_and_partial_progress

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 300 filtered out; finished in 0.33s



thread &#x27;rpc::io::tests::framing_keeps_vectored_writes_through_pending_and_partial_progress&#x27; (32091) panicked at src&#x2F;rpc&#x2F;io.rs:163:13:
vectored writes must reach the transport intact
stack backtrace:
   0: __rustc::rust_begin_unwind
   1: core::panicking::panic_fmt
   2: &lt;capntproto::rpc::io::tests::Writer as tokio::io::async_write::AsyncWrite&gt;::poll_write
   3: &lt;futures_util::io::write_all::WriteAll&lt;&amp;mut capntproto::rpc::io::TokioIo&lt;capntproto::rpc::io::tests::Writer&gt;&gt; as core::future::future::Future&gt;::poll
   4: capnp_futures::serialize::write_framed::&lt;&amp;mut capntproto::rpc::io::TokioIo&lt;capntproto::rpc::io::tests::Writer&gt;&gt;::{closure#0}
   5: capnp_futures::serialize::write_buffers::&lt;&amp;mut capntproto::rpc::io::TokioIo&lt;capntproto::rpc::io::tests::Writer&gt;&gt;::{closure#0}
   6: capnp_futures::serialize::write_message_refs::&lt;&amp;mut capntproto::rpc::io::TokioIo&lt;capntproto::rpc::io::tests::Writer&gt;, capnp::message::Builder&lt;capnp::message::HeapAllocator&gt;, core::slice::iter::Iter&lt;capnp::message::Builder&lt;capnp::message::HeapAllocator&gt;&gt;&gt;::{closure#0}
   7: capnp_futures::serialize::write_messages::&lt;&amp;mut capntproto::rpc::io::TokioIo&lt;capntproto::rpc::io::tests::Writer&gt;, capnp::message::Builder&lt;capnp::message::HeapAllocator&gt;&gt;::{closure#0}
   8: capntproto::rpc::io::tests::framing_keeps_vectored_writes_through_pending_and_partial_progress::{closure#0}
   9: futures_executor::local_pool::run_executor::&lt;(), futures_executor::local_pool::block_on&lt;capntproto::rpc::io::tests::framing_keeps_vectored_writes_through_pending_and_partial_progress::{closure#0}&gt;::{closure#0}&gt;::{closure#0}
  10: futures_executor::local_pool::block_on::&lt;capntproto::rpc::io::tests::framing_keeps_vector</pre>

Diagnostic excerpt truncated; full output is in the workflow artifact.

</details>

<details open>
<summary>2. <code>portable_adoption_exposes_unused_pipeline_before_return_and_preserves_used_route</code></summary>

<p>capntproto::answer_adoption</p>
<pre>thread &#x27;portable_adoption_exposes_unused_pipeline_before_return_and_preserves_used_route&#x27; (32609) panicked at tests&#x2F;answer_adoption.rs:1247:21:
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
  15: tokio::runtime::context::set_scheduler::&lt;(alloc::boxed::Box&lt;tokio::runtime::scheduler::current_thread::Core&gt;, core::option::Option&lt;()&gt;), &lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::enter&lt;&lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::block_on&lt;core::pin::Pin&lt;&amp;mut core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Output = ()&gt;&gt;&gt;&gt;::{closure#0}, core::option::Option&lt;()&gt;&gt;::{closure#0}&gt;::{closure#0}
  16: &lt;std::thread::local::LocalKey&lt;tokio::runtime::context::Context&gt;&gt;::with::&lt;tokio::runtime::context::set_scheduler&lt;(alloc::boxed::Box&lt;tokio::runtime::scheduler::current_thread::Core&gt;, core::option::Option&lt;()&gt;), &lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::enter&lt;&lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::block_on&lt;core::pin::Pin&lt;&amp;mut core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Output = ()&gt;&gt;&gt;&gt;::{closure#0}, core::option::Option&lt;()&gt;&gt;::{closure#0}&gt;::{closure#0}, (alloc::boxed::Box&lt;tokio::runtime::scheduler::current_thread::Core&gt;, core::option::Option&lt;()&gt;)&gt;
  17: tokio::runtime::context::set_scheduler::&lt;(alloc::boxed::Box&lt;tokio::runtime::scheduler::current_thread::Core&gt;, core::option::Option&lt;()&gt;), &lt;tokio::runtime::scheduler::curren</pre>

Diagnostic excerpt truncated; full output is in the workflow artifact.

</details>

<details open>
<summary>3. <code>portable_tail_finish_before_child_completion_reclaims_both_interests</code></summary>

<p>capntproto::answer_adoption</p>
<pre>thread &#x27;portable_tail_finish_before_child_completion_reclaims_both_interests&#x27; (32619) panicked at tests&#x2F;answer_adoption.rs:2577:17:
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
  15: tokio::runtime::context::set_scheduler::&lt;(alloc::boxed::Box&lt;tokio::runtime::scheduler::current_thread::Core&gt;, core::option::Option&lt;()&gt;), &lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::enter&lt;&lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::block_on&lt;core::pin::Pin&lt;&amp;mut core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Output = ()&gt;&gt;&gt;&gt;::{closure#0}, core::option::Option&lt;()&gt;&gt;::{closure#0}&gt;::{closure#0}
  16: &lt;std::thread::local::LocalKey&lt;tokio::runtime::context::Context&gt;&gt;::with::&lt;tokio::runtime::context::set_scheduler&lt;(alloc::boxed::Box&lt;tokio::runtime::scheduler::current_thread::Core&gt;, core::option::Option&lt;()&gt;), &lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::enter&lt;&lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::block_on&lt;core::pin::Pin&lt;&amp;mut core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Output = ()&gt;&gt;&gt;&gt;::{closure#0}, core::option::Option&lt;()&gt;&gt;::{closure#0}&gt;::{closure#0}, (alloc::boxed::Box&lt;tokio::runtime::scheduler::current_thread::Core&gt;, core::option::Option&lt;()&gt;)&gt;
  17: tokio::runtime::context::set_scheduler::&lt;(alloc::boxed::Box&lt;tokio::runtime::scheduler::current_thread::Core&gt;, core::option::Option&lt;()&gt;), &lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::enter&lt;&lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::block_on&lt;core::pin::Pin&lt;&amp;m</pre>

Diagnostic excerpt truncated; full output is in the workflow artifact.

</details>

<details open>
<summary>4. <code>portable_third_party_pipeline_transfer_resumes_after_idle_at_each_poll_budget</code></summary>

<p>capntproto::answer_adoption</p>
<pre>thread &#x27;portable_third_party_pipeline_transfer_resumes_after_idle_at_each_poll_budget&#x27; (32633) panicked at tests&#x2F;answer_adoption.rs:176:37:
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
  12: tokio::runtime::context::set_scheduler::&lt;(alloc::boxed::Box&lt;tokio::runtime::scheduler::current_thread::Core&gt;, core::option::Option&lt;()&gt;), &lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::enter&lt;&lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::block_on&lt;core::pin::Pin&lt;&amp;mut core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Output = ()&gt;&gt;&gt;&gt;::{closure#0}, core::option::Option&lt;()&gt;&gt;::{closure#0}&gt;::{closure#0}
  13: &lt;std::thread::local::LocalKey&lt;tokio::runtime::context::Context&gt;&gt;::with::&lt;tokio::runtime::context::set_scheduler&lt;(alloc::boxed::Box&lt;tokio::runtime::scheduler::current_thread::Core&gt;, core::option::Option&lt;()&gt;), &lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::enter&lt;&lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::block_on&lt;core::pin::Pin&lt;&amp;mut core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Output = ()&gt;&gt;&gt;&gt;::{closure#0}, core::option::Option&lt;()&gt;&gt;::{closure#0}&gt;::{closure#0}, (alloc::boxed::Box&lt;tokio::runtime::scheduler::current_thread::Core&gt;, core::option::Option&lt;()&gt;)&gt;
  14: tokio::runtime::context::set_scheduler::&lt;(alloc::boxed::Box&lt;tokio::runtime::scheduler::current_thread::Core&gt;, core::option::Option&lt;()&gt;), &lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::enter&lt;&lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::block_on&lt;core::pin::Pin&lt;&amp;mut core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Output = ()&gt;&gt;&gt;&gt;::{closure#0}, core::option::Option&lt;()&gt;&gt;::{closure#0}&gt;
  15: &lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::enter::&lt;&lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::block_on&lt;core::pin::Pin&lt;&amp;mut core::pin::Pin&lt;&amp;m</pre>

Diagnostic excerpt truncated; full output is in the workflow artifact.

</details>

<details open>
<summary>5. <code>third_party_tail_returns_over_authenticated_native</code></summary>

<p>capntproto::answer_adoption</p>
<pre>thread &#x27;third_party_tail_returns_over_authenticated_native&#x27; (32694) panicked at tests&#x2F;answer_adoption.rs:1479:37:
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
  12: tokio::runtime::context::set_scheduler::&lt;(alloc::boxed::Box&lt;tokio::runtime::scheduler::current_thread::Core&gt;, core::option::Option&lt;()&gt;), &lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::enter&lt;&lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::block_on&lt;core::pin::Pin&lt;&amp;mut core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Output = ()&gt;&gt;&gt;&gt;::{closure#0}, core::option::Option&lt;()&gt;&gt;::{closure#0}&gt;::{closure#0}
  13: &lt;std::thread::local::LocalKey&lt;tokio::runtime::context::Context&gt;&gt;::with::&lt;tokio::runtime::context::set_scheduler&lt;(alloc::boxed::Box&lt;tokio::runtime::scheduler::current_thread::Core&gt;, core::option::Option&lt;()&gt;), &lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::enter&lt;&lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::block_on&lt;core::pin::Pin&lt;&amp;mut core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Output = ()&gt;&gt;&gt;&gt;::{closure#0}, core::option::Option&lt;()&gt;&gt;::{closure#0}&gt;::{closure#0}, (alloc::boxed::Box&lt;tokio::runtime::scheduler::current_thread::Core&gt;, core::option::Option&lt;()&gt;)&gt;
  14: tokio::runtime::context::set_scheduler::&lt;(alloc::boxed::Box&lt;tokio::runtime::scheduler::current_thread::Core&gt;, core::option::Option&lt;()&gt;), &lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::enter&lt;&lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::block_on&lt;core::pin::Pin&lt;&amp;mut core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Output = ()&gt;&gt;&gt;&gt;::{closure#0}, core::option::Option&lt;()&gt;&gt;::{closure#0}&gt;
  15: &lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::enter::&lt;&lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::block_on&lt;core::pin::Pin&lt;&amp;mut core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Output = ()&gt;&gt;&gt;&gt;::{closure#0}, core::option::Option&lt;()&gt;&gt;
  16: &lt;tokio::runtime::scheduler::current_thread::CurrentThread&gt;::block_on::&lt;core::</pre>

Diagnostic excerpt truncated; full output is in the workflow artifact.

</details>

<details open>
<summary>6. <code>authoritative_absence_and_provisioning_failure_do_not_consult_another_reader</code></summary>

<p>capntproto::native_discovery_failover</p>
<pre>thread &#x27;authoritative_absence_and_provisioning_failure_do_not_consult_another_reader&#x27; (37601) panicked at tests&#x2F;native_discovery_failover.rs:237:17:
assertion failed: connector.connect_name(&quot;service&quot;).await.is_err()
stack backtrace:
   0: __rustc::rust_begin_unwind
   1: core::panicking::panic_fmt
   2: core::panicking::panic
   3: native_discovery_failover::authoritative_absence_and_provisioning_failure_do_not_consult_another_reader::{closure#0}::{closure#0}::{closure#0}
   4: &lt;tokio::time::timeout::Timeout&lt;native_discovery_failover::authoritative_absence_and_provisioning_failure_do_not_consult_another_reader::{closure#0}::{closure#0}::{closure#0}&gt; as core::future::future::Future&gt;::poll
   5: native_discovery_failover::authoritative_absence_and_provisioning_failure_do_not_consult_another_reader::{closure#0}::{closure#0}
   6: &lt;tokio::task::local::RunUntil&lt;native_discovery_failover::authoritative_absence_and_provisioning_failure_do_not_consult_another_reader::{closure#0}::{closure#0}&gt; as core::future::future::Future&gt;::poll::{closure#0}
   7: &lt;tokio::task::local::LocalSet&gt;::with::&lt;core::task::poll::Poll&lt;()&gt;, &lt;tokio::task::local::RunUntil&lt;native_discovery_failover::authoritative_absence_and_provisioning_failure_do_not_consult_another_reader::{closure#0}::{closure#0}&gt; as core::future::future::Future&gt;::poll::{closure#0}&gt;::{closure#0}
   8: &lt;tokio::task::local::LocalSet&gt;::run_until::&lt;native_discovery_failover::authoritative_absence_and_provisioning_failure_do_not_consult_another_reader::{closure#0}::{closure#0}&gt;::{closure#0}
   9: native_discovery_failover::authoritative_absence_and_provisioning_failure_do_not_consult_another_reader::{closure#0}
  10: tokio::task::coop::budget::&lt;core::task::poll::Poll&lt;()&gt;, &lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::block_on&lt;core::pin::Pin&lt;&amp;mut core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Output = ()&gt;&gt;&gt;&gt;::{closure#0}::{closure#0}::{closure#0}&gt;
  11: &lt;tokio::runtime::scheduler::current_thread::Context&gt;::enter::&lt;core::task::poll::Poll&lt;()&gt;, &lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::block_on&lt;core::pin::Pin&lt;&amp;mut core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Output = ()&gt;&gt;&gt;&gt;::{closure#0}::{closure#0}&gt;
  12: &lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::block_on::&lt;core::pin::Pin&lt;&amp;mut core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Output = ()&gt;&gt;&gt;&gt;::{closure#0}
  13: &lt;tokio::runtime::context::scoped::Scoped&lt;tokio::runtime::scheduler::Context&gt;&gt;::set::&lt;&lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::enter&lt;&lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::block_on&lt;core::pin::Pin&lt;&amp;mut core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Output = ()&gt;&gt;&gt;&gt;::{closure#0}, core::option::Option&lt;()&gt;&gt;::{closure#0}, (alloc::boxed::Box&lt;tokio::runtime::scheduler::current_thread::Core&gt;, core::option::Option&lt;()&gt;)&gt;
  14: tokio::runtime::context::set_scheduler::&lt;(alloc::boxed::Box&lt;tokio::runtime::scheduler::current_thread::Core&gt;, core::option::Option&lt;()&gt;), &lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::enter&lt;&lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::block_on&lt;core::pin::Pin&lt;&amp;mut core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Output = ()&gt;&gt;&gt;&gt;::{closure#0}, core::option::Option&lt;()&gt;&gt;::{closure#0}&gt;::{closure#0}
  15: &lt;std::thread::local::LocalKey&lt;tokio::runtime::context::Context&gt;&gt;::with::&lt;tokio::runtime::context::set_scheduler&lt;(alloc::boxed::Box&lt;tokio::runtime::scheduler::current_thread::Core&gt;, core::option::Option&lt;()&gt;), &lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::enter&lt;&lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::block_on&lt;core::pin::Pin&lt;&amp;mut core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Output = ()&gt;&gt;&gt;&gt;::{closure#0}, core::option::Option&lt;()&gt;&gt;::{closure#0}&gt;::{closure#0}, (alloc::boxed::Box&lt;tokio::runtime::scheduler::current_thread::Core&gt;, core::option::Option&lt;()&gt;)&gt;
  16: tokio::runtime::context::set_scheduler::&lt;(alloc::boxed::Box&lt;tokio::runtime::scheduler::current_thread::Core&gt;, core::option::Option&lt;()&gt;), &lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::enter&lt;&lt;tokio::runtime::scheduler::current_t</pre>

Diagnostic excerpt truncated; full output is in the workflow artifact.

</details>

<details open>
<summary>7. <code>report::tests::report_appends_charts_and_removes_them_when_samples_are_tampered</code></summary>

<p>capntproto-quality</p>
<pre>thread &#x27;report::tests::report_appends_charts_and_removes_them_when_samples_are_tampered&#x27; (42692) panicked at quality&#x2F;src&#x2F;report.rs:481:9:
1 quality requirements failed:
- legacy TCP&#x2F;C++ minimum exceeds 1.15x at 0 bytes: 50.0000x
See &#x2F;tmp&#x2F;.tmpUMQjHz&#x2F;README.md
stack backtrace:
   0: __rustc::rust_begin_unwind
   1: core::panicking::panic_fmt
   2: capntproto_quality::report::tests::report_appends_charts_and_removes_them_when_samples_are_tampered
   3: &lt;capntproto_quality::report::tests::report_appends_charts_and_removes_them_when_samples_are_tampered::{closure#0} as core::ops::function::FnOnce&lt;()&gt;&gt;::call_once
note: Some details are omitted, run with `RUST_BACKTRACE=full` for a verbose backtrace.

running 1 test
test report::tests::report_appends_charts_and_removes_them_when_samples_are_tampered ... FAILED

failures:

failures:
    report::tests::report_appends_charts_and_removes_them_when_samples_are_tampered

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 21 filtered out; finished in 1.57s



thread &#x27;report::tests::report_appends_charts_and_removes_them_when_samples_are_tampered&#x27; (42692) panicked at quality&#x2F;src&#x2F;report.rs:481:9:
1 quality requirements failed:
- legacy TCP&#x2F;C++ minimum exceeds 1.15x at 0 bytes: 50.0000x
See &#x2F;tmp&#x2F;.tmpUMQjHz&#x2F;README.md
stack backtrace:
   0: __rustc::rust_begin_unwind
   1: core::panicking::panic_fmt
   2: capntproto_quality::report::tests::report_appends_charts_and_removes_them_when_samples_are_tampered
   3: &lt;capntproto_quality::report::tests::report_appends_charts_and_removes_them_when_samples_are_tampered::{closure#0} as core::ops::function::FnOnce&lt;()&gt;&gt;::call_once
note: Some details are omitted, run with `RUST_BACKTRACE=full` for a verbose backtrace.
</pre>

</details>

<details open>
<summary>8. <code>formatting_and_lints</code></summary>

<p>capntproto::tooling</p>
<pre>thread &#x27;formatting_and_lints&#x27; (73022) panicked at tests&#x2F;tooling.rs:60:44:
called `Result::unwrap()` on an `Err` value: &quot;cd \&quot;&#x2F;home&#x2F;runner&#x2F;work&#x2F;capntproto&#x2F;capntproto\&quot; &amp;&amp; env -u CAPNTPROTO_COVERAGE_CHILDREN_NATIVE -u CARGO_ENCODED_RUSTFLAGS -u CARGO_TARGET_DIR -u LLVM_PROFILE_FILE -u NEXTEST_PROFILE -u RUSTDOCFLAGS -u RUSTFLAGS -u RUSTUP_TOOLCHAIN \&quot;cargo\&quot; \&quot;fmt\&quot; \&quot;--manifest-path\&quot; \&quot;benchmarks&#x2F;rpc&#x2F;Cargo.toml\&quot; \&quot;--\&quot; \&quot;--check\&quot;: expected 0, got exit status: 1; inspect &#x2F;home&#x2F;runner&#x2F;work&#x2F;capntproto&#x2F;capntproto&#x2F;target&#x2F;verification&#x2F;tooling&#x2F;quality&#x2F;benchmark-format.log\nDiff in &#x2F;home&#x2F;runner&#x2F;work&#x2F;capntproto&#x2F;capntproto&#x2F;benchmarks&#x2F;rpc&#x2F;src&#x2F;..&#x2F;tls_policy.rs:1:\n &#x2F;&#x2F;! Shared benchmark certificate and TLS policy; identical across Rust drivers.\n+use super::rustls;\n use crate::Result;\n use rustls::pki_types::{CertificateDer, PrivateKeyDer};\n use std::{path::PathBuf, sync::Arc, time::Duration};\nDiff in &#x2F;home&#x2F;runner&#x2F;work&#x2F;capntproto&#x2F;capntproto&#x2F;benchmarks&#x2F;rpc&#x2F;src&#x2F;..&#x2F;tls_policy.rs:5:\n-use super::rustls;\n \n pub const DEADLINE: Duration = Duration::from_secs(10);\n pub fn directory() -&gt; Result&lt;PathBuf&gt; {\nDiff in &#x2F;home&#x2F;runner&#x2F;work&#x2F;capntproto&#x2F;capntproto&#x2F;benchmarks&#x2F;rpc&#x2F;src&#x2F;topology.rs:115:\n     let mut driver = capnp_rpc::RpcSystem::new(network, None);\n     let client = driver.bootstrap(capnp_rpc::rpc_twoparty_capnp::Side::Server);\n     tokio::task::spawn_local(async move {\n-        if let Err(error) = driver.await { eprintln!(\&quot;topology downstream RPC: {error}\&quot;); }\n+        if let Err(error) = driver.await {\n+            eprintln!(\&quot;topology downstream RPC: {error}\&quot;);\n+        }\n     });\n     Ok(client)\n }\nDiff in &#x2F;home&#x2F;runner&#x2F;work&#x2F;capntproto&#x2F;capntproto&#x2F;benchmarks&#x2F;rpc&#x2F;src&#x2F;topology.rs:172:\n         } else {\n             let driver = capnp_rpc::RpcSystem::new(network, Some(service.client.clone()));\n             tokio::task::spawn_local(async move {\n-                if let Err(error) = driver.await { eprintln!(\&quot;topology service RPC: {error}\&quot;); }\n+                if let Err(error) = driver.await {\n+                    eprintln!(\&quot;topology service RPC: {error}\&quot;);\n+                }\n             });\n         }\n     }\n&quot;
stack backtrace:
   0: __rustc::rust_begin_unwind
   1: core::panicking::panic_fmt
   2: core::result::unwrap_failed
   3: tooling::cargo_with_toolchain
   4: tooling::formatting_and_lints
   5: &lt;tooling::formatting_and_lints::{closure#0} as core::ops::function::FnOnce&lt;()&gt;&gt;::call_once
note: Some details are omitted, run with `RUST_BACKTRACE=full` for a verbose backtrace.

running 1 test
test formatting_and_lints ... FAILED

failures:

failures:
    formatting_and_lints

test result: FAILED. 0 passed; 1 failed; 0 ignored; 0 measured; 17 filtered out; finished in 5.05s



thread &#x27;formatting_and_lints&#x27; (73022) panicked at tests&#x2F;tooling.rs:60:44:
called `Result::unwrap()` on an `Err` value: &quot;cd \&quot;&#x2F;home&#x2F;runner&#x2F;work&#x2F;capntproto&#x2F;capntproto\&quot; &amp;&amp; env -u CAPNTPROTO_COVERAGE_CHILDREN_NATIVE -u CARGO_ENCODED_RUSTFLAGS -u CARGO_TARGET_DIR -u LLVM_PROFILE_FILE -u NEXTEST_PROFILE -u RUSTDOCFLAGS -u RUSTFLAGS -u RUSTUP_TOOLCHAIN \&quot;cargo\&quot; \&quot;fmt\&quot; \&quot;--manifest-path\&quot; \&quot;benchmarks&#x2F;rpc&#x2F;Cargo.toml\&quot; \&quot;--\&quot; \&quot;--check\&quot;: expected 0, got exit status: 1; inspect &#x2F;home&#x2F;runner&#x2F;work&#x2F;capntproto&#x2F;capntproto&#x2F;target&#x2F;verification&#x2F;tooling&#x2F;quality&#x2F;benchmark-format.log\nDiff in &#x2F;home&#x2F;runner&#x2F;work&#x2F;capntproto&#x2F;capntproto&#x2F;benchmarks&#x2F;rpc&#x2F;src&#x2F;..&#x2F;tls_policy.rs:1:\n &#x2F;&#x2F;! Shared benchmark certificate and TLS policy; identical across Rust drivers.\n+use super::rustls;\n use crate::Result;\n use rustls::pki_types::{CertificateDer, PrivateKeyDer};\n use std::{path::PathBuf, sync::Arc, time::Duration};\nDiff in &#x2F;home&#x2F;runner&#x2F;work&#x2F;capntproto&#x2F;capntproto&#x2F;benchmarks&#x2F;rpc&#x2F;src&#x2F;..&#x2F;tls_policy.rs:5:\n-use super::rustls;\n \n pub const DEADLINE: Duration = Duration::from_secs(10);\n pub fn directory() -&gt; Result&lt;PathBuf&gt; {\nDiff in &#x2F;home&#x2F;runner&#x2F;work&#x2F;capntproto&#x2F;capntproto&#x2F;benchmarks&#x2F;rpc&#x2F;src&#x2F;topology.rs:115:\n     let mut driver = capnp_rpc::RpcSystem::new(network, None);\n     let client = driver.bootstrap(capnp_rpc::rpc_twoparty_capnp::Side::Server);\n     toki</pre>

Diagnostic excerpt truncated; full output is in the workflow artifact.

</details>

<details open>
<summary>9. <code>pinned_cpp_runtime_reference</code></summary>

<p>capntproto::tooling</p>
<pre>thread &#x27;pinned_cpp_runtime_reference&#x27; (101539) panicked at tests&#x2F;tooling.rs:259:25:
called `Result::unwrap()` on an `Err` value: &quot;cd \&quot;&#x2F;home&#x2F;runner&#x2F;work&#x2F;capntproto&#x2F;capntproto\&quot; &amp;&amp; env -u CAPNTPROTO_COVERAGE_CHILDREN_NATIVE -u CARGO_ENCODED_RUSTFLAGS -u CARGO_TARGET_DIR -u LLVM_PROFILE_FILE -u NEXTEST_PROFILE -u RUSTDOCFLAGS -u RUSTFLAGS -u RUSTUP_TOOLCHAIN \&quot;clang++-22\&quot; \&quot;-std=c++23\&quot; \&quot;-DCAPNTPROTO_PINNED_RUNTIME\&quot; \&quot;-I&#x2F;tmp&#x2F;.tmpQEPju3\&quot; \&quot;-Ivendor&#x2F;capnproto&#x2F;c++&#x2F;src\&quot; \&quot;tests&#x2F;cpp&#x2F;static-cancellation.c++\&quot; \&quot;&#x2F;tmp&#x2F;.tmpQEPju3&#x2F;runtime-test.capnp.c++\&quot; \&quot;&#x2F;tmp&#x2F;.tmpQEPju3&#x2F;cancellation-policy.capnp.c++\&quot; \&quot;&#x2F;tmp&#x2F;.tmpQEPju3&#x2F;dynamic-test.capnp.c++\&quot; \&quot;&#x2F;home&#x2F;runner&#x2F;work&#x2F;capntproto&#x2F;capntproto&#x2F;target&#x2F;cpp-reference&#x2F;c++&#x2F;src&#x2F;capnp&#x2F;libcapnp-rpc.a\&quot; \&quot;&#x2F;home&#x2F;runner&#x2F;work&#x2F;capntproto&#x2F;capntproto&#x2F;target&#x2F;cpp-reference&#x2F;c++&#x2F;src&#x2F;capnp&#x2F;libcapnp.a\&quot; \&quot;&#x2F;home&#x2F;runner&#x2F;work&#x2F;capntproto&#x2F;capntproto&#x2F;target&#x2F;cpp-reference&#x2F;c++&#x2F;src&#x2F;kj&#x2F;libkj-async.a\&quot; \&quot;&#x2F;home&#x2F;runner&#x2F;work&#x2F;capntproto&#x2F;capntproto&#x2F;target&#x2F;cpp-reference&#x2F;c++&#x2F;src&#x2F;kj&#x2F;libkj.a\&quot; \&quot;-pthread\&quot; \&quot;-o\&quot; \&quot;&#x2F;tmp&#x2F;.tmpQEPju3&#x2F;reference\&quot;: expected 0, got exit status: 1; inspect &#x2F;home&#x2F;runner&#x2F;work&#x2F;capntproto&#x2F;capntproto&#x2F;target&#x2F;verification&#x2F;cpp-reference&#x2F;compile.log\nl be removed. \&quot; \\\n      |       ^\ntests&#x2F;cpp&#x2F;static-cancellation.c++:79:64: warning: &#x27;Maybe&#x27; is deprecated: Using nullptr as an empty Maybe is deprecated and will be removed. Please use kj::none for this purpose instead. [-Wdeprecated-declarations]\n   79 |     auto promise = client.typelessRequest(interfaceId, method, nullptr, {}).send();\n      |                                                                ^\ntests&#x2F;cpp&#x2F;static-cancellation.c++:388:3: note: in instantiation of function template specialization &#x27;check&lt;DerivedServer&gt;&#x27; requested here\n  388 |   check&lt;DerivedServer&gt;(wait, capnp::typeId&lt;Policy&gt;(), 0, false);\n      |   ^\nvendor&#x2F;capnproto&#x2F;c++&#x2F;src&#x2F;kj&#x2F;common.h:1925:3: note: &#x27;Maybe&#x27; has been explicitly marked deprecated here\n 1925 |   KJ_DEPRECATE_EMPTY_MAYBE_FROM_NULLPTR_ATTR\n      |   ^\nvendor&#x2F;capnproto&#x2F;c++&#x2F;src&#x2F;kj&#x2F;common.h:1766:7: note: expanded from macro &#x27;KJ_DEPRECATE_EMPTY_MAYBE_FROM_NULLPTR_ATTR&#x27;\n 1766 |     [[deprecated(\&quot;Using nullptr as an empty Maybe is deprecated and will be removed. \&quot; \\\n      |       ^\ntests&#x2F;cpp&#x2F;static-cancellation.c++:79:64: warning: &#x27;Maybe&#x27; is deprecated: Using nullptr as an empty Maybe is deprecated and will be removed. Please use kj::none for this purpose instead. [-Wdeprecated-declarations]\n   79 |     auto promise = client.typelessRequest(interfaceId, method, nullptr, {}).send();\n      |                                                                ^\ntests&#x2F;cpp&#x2F;static-cancellation.c++:390:3: note: in instantiation of function template specialization &#x27;check&lt;FileServer&gt;&#x27; requested here\n  390 |   check&lt;FileServer&gt;(wait, capnp::typeId&lt;Harness&gt;(), 3, true);\n      |   ^\nvendor&#x2F;capnproto&#x2F;c++&#x2F;src&#x2F;kj&#x2F;common.h:1925:3: note: &#x27;Maybe&#x27; has been explicitly marked deprecated here\n 1925 |   KJ_DEPRECATE_EMPTY_MAYBE_FROM_NULLPTR_ATTR\n      |   ^\nvendor&#x2F;capnproto&#x2F;c++&#x2F;src&#x2F;kj&#x2F;common.h:1766:7: note: expanded from macro &#x27;KJ_DEPRECATE_EMPTY_MAYBE_FROM_NULLPTR_ATTR&#x27;\n 1766 |     [[deprecated(\&quot;Using nullptr as an empty Maybe is deprecated and will be removed. \&quot; \\\n      |       ^\n6 warnings and 3 errors generated.\nIn file included from &#x2F;tmp&#x2F;.tmpQEPju3&#x2F;dynamic-test.capnp.c++:4:\n&#x2F;tmp&#x2F;.tmpQEPju3&#x2F;dynamic-test.capnp.h:8408:32: error: use &#x27;template&#x27; keyword to treat &#x27;Inner&#x27; as a dependent template name\n 8408 | inline typename GroupScope&lt;A&gt;::Inner&lt;B, C&gt;::Payload::Reader GroupScope&lt;A&gt;::Inner&lt;B, C&gt;::Reader::getPayload() const {\n      |                                ^\n&#x2F;tmp&#x2F;.tmpQEPju3&#x2F;dynamic-test.capnp.h:8415:32: error: use &#x27;template&#x27; keyword to treat &#x27;Inner&#x27; as a dependent template name\n 8415 | inline typename GroupScope&lt;A&gt;::Inner&lt;B, C&gt;::Payload::Builder GroupScope&lt;A&gt;::Inner&lt;B, C&gt;::Builder::getPayload() {\n      |                                ^\n&#x2F;tmp&#x2F;.tmpQEPju3&#x2F;dynamic-test.capnp.h:8422:32: error: use &#x27;template&#x27; keyword to treat &#x27;Inner&#x27; as a dependent template name\n 8422 | inline typename GroupScope&lt;A&gt;::Inner&lt;B, C&gt;::</pre>

Diagnostic excerpt truncated; full output is in the workflow artifact.

</details>

The workspace command failed or was incomplete. Build/infrastructure errors and tests that never finished are not invented as named failures.
