@0xca272652a0261223;
using Cxx = import "/capnp/c++.capnp";
using Harness = import "runtime-test.capnp".Harness;
interface Policy {
  pending @0 (earlyDrop :Bool) -> (cap :Harness);
  cancellable @1 (earlyDrop :Bool) -> (cap :Harness) $Cxx.allowCancellation;
  stream @2 () -> stream;
  cancellableStream @3 () -> stream $Cxx.allowCancellation;
}
interface Allowed $Cxx.allowCancellation {
  pending @0 (earlyDrop :Bool) -> (cap :Harness);
}
interface Derived extends(Policy) {
  pendingOwn @0 (earlyDrop :Bool) -> (cap :Harness) $Cxx.allowCancellation;
}
