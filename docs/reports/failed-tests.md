# Failed workspace tests

[Workflow run and full logs](https://github.com/DericHuynh/capntproto/actions/runs/37082380827) · commit `390261cd8481ea9b5de64db4c7eceb8cf36fb289` · attempt 1

Recorded: **6 failed**, **0 without a terminal result**, 1354 passed and 11 skipped.

<details open>
<summary>1. <code>native_shutdown::tests::replay_tlc_native_shutdown_traces</code></summary>

<p>Running unittests src/lib.rs (target/quality-build/40fa5a253080142d06445e66cd1efbbdfd8486718728d16b12d33694743fc517/root/debug/deps/reproto-3e33aaf556e34dc5)</p>
<pre>TLC native-shutdown / binding
TLC native-shutdown / bytes-0-crossed-0/model-liveness
TLC native-shutdown / bytes-0-crossed-0/model
TLC native-shutdown / bytes-2-crossed-0/model-liveness
TLC native-shutdown / bytes-2-crossed-0/model
TLC native-shutdown / bytes-2-crossed-1/model-liveness
TLC native-shutdown / bytes-2-crossed-1/model
TLC native-shutdown / crossed
TLC native-shutdown / earlyAck
TLC native-shutdown / earlyReply
TLC native-shutdown / resurrect
TLC native-shutdown / undrained
Task failed, serializing schedule
test panicked in task &#x27;Tried to get ExecutionState, but got the following error: NotSet&#x27;

thread &#x27;native_shutdown::tests::replay_tlc_native_shutdown_traces&#x27; (36712) panicked at test-support/src/traces.rs:16:5:
assertion `left == right` failed: wrong model input
  left: &quot;RpcNoiseShutdown&quot;
 right: &quot;RpcNativeShutdown&quot;
stack backtrace:
   0: __rustc::rust_begin_unwind
   1: core::panicking::panic_fmt
   2: core::panicking::assert_failed_inner
   3: core::panicking::assert_failed::&lt;alloc::string::String, &amp;str&gt;
   4: reproto_test_support::traces::read::&lt;reproto::native_shutdown::tests::Trace, alloc::string::String&gt;
   5: reproto::native_shutdown::tests::replay_tlc_native_shutdown_traces::{closure#0}
   6: tokio::task::coop::budget::&lt;core::task::poll::Poll&lt;()&gt;, &lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::block_on&lt;core::pin::Pin&lt;&amp;mut core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Output = ()&gt;&gt;&gt;&gt;::{closure#0}::{closure#0}::{closure#0}&gt;
   7: &lt;tokio::runtime::scheduler::current_thread::Context&gt;::enter::&lt;core::task::poll::Poll&lt;()&gt;, &lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::block_on&lt;core::pin::Pin&lt;&amp;mut core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Output = ()&gt;&gt;&gt;&gt;::{closure#0}::{closure#0}&gt;
   8: &lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::block_on::&lt;core::pin::Pin&lt;&amp;mut core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Output = ()&gt;&gt;&gt;&gt;::{closure#0}
   9: &lt;tokio::runtime::context::scoped::Scoped&lt;tokio::runtime::scheduler::Context&gt;&gt;::set::&lt;&lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::enter&lt;&lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::block_on&lt;core::pin::Pin&lt;&amp;mut core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Output = ()&gt;&gt;&gt;&gt;::{closure#0}, core::option::Option&lt;()&gt;&gt;::{closure#0}, (alloc::boxed::Box&lt;tokio::runtime::scheduler::current_thread::Core&gt;, core::option::Option&lt;()&gt;)&gt;
  10: tokio::runtime::context::set_scheduler::&lt;(alloc::boxed::Box&lt;tokio::runtime::scheduler::current_thread::Core&gt;, core::option::Option&lt;()&gt;), &lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::enter&lt;&lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::block_on&lt;core::pin::Pin&lt;&amp;mut core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Output = ()&gt;&gt;&gt;&gt;::{closure#0}, core::option::Option&lt;()&gt;&gt;::{closure#0}&gt;::{closure#0}
  11: &lt;std::thread::local::LocalKey&lt;tokio::runtime::context::Context&gt;&gt;::with::&lt;tokio::runtime::context::set_scheduler&lt;(alloc::boxed::Box&lt;tokio::runtime::scheduler::current_thread::Core&gt;, core::option::Option&lt;()&gt;), &lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::enter&lt;&lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::block_on&lt;core::pin::Pin&lt;&amp;mut core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Output = ()&gt;&gt;&gt;&gt;::{closure#0}, core::option::Option&lt;()&gt;&gt;::{closure#0}&gt;::{closure#0}, (alloc::boxed::Box&lt;tokio::runtime::scheduler::current_thread::Core&gt;, core::option::Option&lt;()&gt;)&gt;
  12: tokio::runtime::context::set_scheduler::&lt;(alloc::boxed::Box&lt;tokio::runtime::scheduler::current_thread::Core&gt;, core::option::Option&lt;()&gt;), &lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::enter&lt;&lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::block_on&lt;core::pin::Pin&lt;&amp;mut core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Output = ()&gt;&gt;&gt;&gt;::{closure#0}, core::option::Option&lt;()&gt;&gt;::{closure#0}&gt;
  13: &lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::enter::&lt;&lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::block_on&lt;core::pin::Pin&lt;&amp;mut core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Output = ()&gt;&gt;&gt;&gt;::{closure#0}, core::option::Option&lt;()&gt;&gt;
  14: </pre>

Diagnostic excerpt truncated; full output is in the workflow artifact.

</details>

<details open>
<summary>2. <code>tlc_authenticated_handoff_traces</code></summary>

<p>Running tests/handoff_auth.rs (target/quality-build/40fa5a253080142d06445e66cd1efbbdfd8486718728d16b12d33694743fc517/root/debug/deps/handoff_auth-249cff7a874e1325)</p>
<pre>fresh TLC exploration: 158 states, 236 edge-prefix traces

thread &#x27;tlc_authenticated_handoff_traces&#x27; (85694) panicked at tests/handoff_auth.rs:360:34:
authentication deadline: Elapsed(())
stack backtrace:
   0: __rustc::rust_begin_unwind
   1: core::panicking::panic_fmt
   2: core::result::unwrap_failed
   3: handoff_auth::tlc_authenticated_handoff_traces::{closure#0}::{closure#0}
   4: &lt;tokio::task::local::RunUntil&lt;handoff_auth::tlc_authenticated_handoff_traces::{closure#0}::{closure#0}&gt; as core::future::future::Future&gt;::poll::{closure#0}
   5: &lt;tokio::task::local::LocalSet&gt;::with::&lt;core::task::poll::Poll&lt;()&gt;, &lt;tokio::task::local::RunUntil&lt;handoff_auth::tlc_authenticated_handoff_traces::{closure#0}::{closure#0}&gt; as core::future::future::Future&gt;::poll::{closure#0}&gt;::{closure#0}
   6: &lt;tokio::task::local::LocalSet&gt;::run_until::&lt;handoff_auth::tlc_authenticated_handoff_traces::{closure#0}::{closure#0}&gt;::{closure#0}
   7: handoff_auth::tlc_authenticated_handoff_traces::{closure#0}
   8: tokio::task::coop::budget::&lt;core::task::poll::Poll&lt;()&gt;, &lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::block_on&lt;core::pin::Pin&lt;&amp;mut core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Output = ()&gt;&gt;&gt;&gt;::{closure#0}::{closure#0}::{closure#0}&gt;
   9: &lt;tokio::runtime::scheduler::current_thread::Context&gt;::enter::&lt;core::task::poll::Poll&lt;()&gt;, &lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::block_on&lt;core::pin::Pin&lt;&amp;mut core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Output = ()&gt;&gt;&gt;&gt;::{closure#0}::{closure#0}&gt;
  10: &lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::block_on::&lt;core::pin::Pin&lt;&amp;mut core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Output = ()&gt;&gt;&gt;&gt;::{closure#0}
  11: &lt;tokio::runtime::context::scoped::Scoped&lt;tokio::runtime::scheduler::Context&gt;&gt;::set::&lt;&lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::enter&lt;&lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::block_on&lt;core::pin::Pin&lt;&amp;mut core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Output = ()&gt;&gt;&gt;&gt;::{closure#0}, core::option::Option&lt;()&gt;&gt;::{closure#0}, (alloc::boxed::Box&lt;tokio::runtime::scheduler::current_thread::Core&gt;, core::option::Option&lt;()&gt;)&gt;
  12: tokio::runtime::context::set_scheduler::&lt;(alloc::boxed::Box&lt;tokio::runtime::scheduler::current_thread::Core&gt;, core::option::Option&lt;()&gt;), &lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::enter&lt;&lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::block_on&lt;core::pin::Pin&lt;&amp;mut core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Output = ()&gt;&gt;&gt;&gt;::{closure#0}, core::option::Option&lt;()&gt;&gt;::{closure#0}&gt;::{closure#0}
  13: &lt;std::thread::local::LocalKey&lt;tokio::runtime::context::Context&gt;&gt;::with::&lt;tokio::runtime::context::set_scheduler&lt;(alloc::boxed::Box&lt;tokio::runtime::scheduler::current_thread::Core&gt;, core::option::Option&lt;()&gt;), &lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::enter&lt;&lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::block_on&lt;core::pin::Pin&lt;&amp;mut core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Output = ()&gt;&gt;&gt;&gt;::{closure#0}, core::option::Option&lt;()&gt;&gt;::{closure#0}&gt;::{closure#0}, (alloc::boxed::Box&lt;tokio::runtime::scheduler::current_thread::Core&gt;, core::option::Option&lt;()&gt;)&gt;
  14: tokio::runtime::context::set_scheduler::&lt;(alloc::boxed::Box&lt;tokio::runtime::scheduler::current_thread::Core&gt;, core::option::Option&lt;()&gt;), &lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::enter&lt;&lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::block_on&lt;core::pin::Pin&lt;&amp;mut core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Output = ()&gt;&gt;&gt;&gt;::{closure#0}, core::option::Option&lt;()&gt;&gt;::{closure#0}&gt;
  15: &lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::enter::&lt;&lt;tokio::runtime::scheduler::current_thread::CoreGuard&gt;::block_on&lt;core::pin::Pin&lt;&amp;mut core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Output = ()&gt;&gt;&gt;&gt;::{closure#0}, core::option::Option&lt;()&gt;&gt;
  16: &lt;tokio::runtime::scheduler::current_thread::CurrentThread&gt;::block_on::&lt;core::pin::Pin&lt;&amp;mut dyn core::future::future::Future&lt;Output = ()&gt;&gt;&gt;::{closure#0}
  17: tokio::runtime::context::runtime::enter_runtime::&lt;&lt;tokio::runtime::scheduler::</pre>

Diagnostic excerpt truncated; full output is in the workflow artifact.

</details>

<details open>
<summary>3. <code>native_fuzz_smoke</code></summary>

<p>Running tests/native_fuzz.rs (target/quality-build/40fa5a253080142d06445e66cd1efbbdfd8486718728d16b12d33694743fc517/root/debug/deps/native_fuzz-54a000fd80b88a2d)</p>
<pre>
thread &#x27;native_fuzz_smoke&#x27; (92015) panicked at tests/native_fuzz.rs:172:9:
assertion `left == right` failed
  left: 13552
 right: 13321
stack backtrace:
   0: __rustc::rust_begin_unwind
   1: core::panicking::panic_fmt
   2: core::panicking::assert_failed_inner
   3: core::panicking::assert_failed::&lt;usize, usize&gt;
   4: native_fuzz::native_fuzz_smoke
   5: &lt;native_fuzz::native_fuzz_smoke::{closure#0} as core::ops::function::FnOnce&lt;()&gt;&gt;::call_once
note: Some details are omitted, run with `RUST_BACKTRACE=full` for a verbose backtrace.
</pre>

</details>

<details open>
<summary>4. <code>annotations::annotation_imports_and_dependencies_match_pinned_cpp</code></summary>

<p>Running tests/schema_compiler.rs (target/quality-build/40fa5a253080142d06445e66cd1efbbdfd8486718728d16b12d33694743fc517/root/debug/deps/schema_compiler-b7153b14325f529e)</p>
<pre>
thread &#x27;annotations::annotation_imports_and_dependencies_match_pinned_cpp&#x27; (118607) panicked at tests/schema_compiler/annotations.rs:309:238:
called `Result::unwrap()` on an `Err` value: Os { code: 2, kind: NotFound, message: &quot;No such file or directory&quot; }
stack backtrace:
   0: __rustc::rust_begin_unwind
   1: core::panicking::panic_fmt
   2: core::result::unwrap_failed
   3: &lt;core::result::Result&lt;(), std::io::error::Error&gt;&gt;::unwrap
   4: schema_compiler::annotations::annotation_imports_and_dependencies_match_pinned_cpp
   5: &lt;schema_compiler::annotations::annotation_imports_and_dependencies_match_pinned_cpp::{closure#0} as core::ops::function::FnOnce&lt;()&gt;&gt;::call_once
note: Some details are omitted, run with `RUST_BACKTRACE=full` for a verbose backtrace.
</pre>

</details>

<details open>
<summary>5. <code>annotations::annotations_match_pinned_cpp</code></summary>

<p>Running tests/schema_compiler.rs (target/quality-build/40fa5a253080142d06445e66cd1efbbdfd8486718728d16b12d33694743fc517/root/debug/deps/schema_compiler-b7153b14325f529e)</p>
<pre>
thread &#x27;annotations::annotations_match_pinned_cpp&#x27; (118643) panicked at tests/schema_compiler/annotations.rs:147:228:
called `Result::unwrap()` on an `Err` value: Os { code: 2, kind: NotFound, message: &quot;No such file or directory&quot; }
stack backtrace:
   0: __rustc::rust_begin_unwind
   1: core::panicking::panic_fmt
   2: core::result::unwrap_failed
   3: &lt;core::result::Result&lt;(), std::io::error::Error&gt;&gt;::unwrap
   4: schema_compiler::annotations::annotations_match_pinned_cpp
   5: &lt;schema_compiler::annotations::annotations_match_pinned_cpp::{closure#0} as core::ops::function::FnOnce&lt;()&gt;&gt;::call_once
note: Some details are omitted, run with `RUST_BACKTRACE=full` for a verbose backtrace.
</pre>

</details>

<details open>
<summary>6. <code>external_consumer_default_features</code></summary>

<p>Running tests/tooling.rs (target/quality-build/40fa5a253080142d06445e66cd1efbbdfd8486718728d16b12d33694743fc517/root/debug/deps/tooling-aa69071aa70d5aff)</p>
<pre>
thread &#x27;external_consumer_default_features&#x27; (126586) panicked at tests/tooling.rs:673:10:
called `Result::unwrap()` on an `Err` value: &quot;cd \&quot;/tmp/.tmpMWYFTb\&quot; &amp;&amp; env -u CARGO_ENCODED_RUSTFLAGS -u LLVM_PROFILE_FILE -u REPROTO_COVERAGE_CHILDREN_NATIVE -u RUSTDOCFLAGS -u RUSTFLAGS -u RUSTUP_TOOLCHAIN CARGO_TARGET_DIR=\&quot;/home/runner/work/capntproto/capntproto/target/downstream-check\&quot; \&quot;cargo\&quot; \&quot;metadata\&quot; \&quot;--locked\&quot; \&quot;--offline\&quot; \&quot;--format-version\&quot; \&quot;1\&quot; \&quot;--manifest-path\&quot; \&quot;/tmp/.tmpMWYFTb/Cargo.toml\&quot;: expected 0, got exit status: 101; inspect /home/runner/work/capntproto/capntproto/target/verification/tooling/downstream/metadata.log\nerror: failed to download `r-efi v6.0.0`\n\nCaused by:\n  attempting to make an HTTP request, but --offline was specified\n&quot;
stack backtrace:
   0: __rustc::rust_begin_unwind
   1: core::panicking::panic_fmt
   2: core::result::unwrap_failed
   3: tooling::external_consumer_default_features::{closure#1}
   4: tooling::external_consumer_default_features
   5: &lt;tooling::external_consumer_default_features::{closure#0} as core::ops::function::FnOnce&lt;()&gt;&gt;::call_once
note: Some details are omitted, run with `RUST_BACKTRACE=full` for a verbose backtrace.
</pre>

</details>

The workspace command failed or was incomplete. Build/infrastructure errors and tests that never finished are not invented as named failures.
