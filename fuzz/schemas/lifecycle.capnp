@0xe8405b90169f72ab;
using Cxx = import "/capnp/c++.capnp";
$Cxx.allowCancellation;

interface Worker {
  create @0 () -> (cap :Worker);
  echo @1 (value :UInt32) -> (value :UInt32);
  retain @2 () -> (cap :Worker);
  wait @3 (token :UInt32) -> (value :UInt32, cap :Worker);
  promise @4 (token :UInt32) -> (cap :Worker);
}
