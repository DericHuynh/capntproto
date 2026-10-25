@0xb8067cae5413ef81;
using Cxx = import "/capnp/c++.capnp";
interface Node {
  read @0 (value :UInt64) -> (value :UInt64);
  next @1 () -> (child :Node);
  callback @2 (target :Node, value :UInt64) -> (value :UInt64);
  hold @3 () -> (value :UInt64) $Cxx.allowCancellation;
}
