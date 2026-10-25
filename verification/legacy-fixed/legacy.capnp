@0xaebf847e65ec1192;
using Cxx = import "/capnp/c++.capnp";
interface Base $Cxx.allowCancellation {
  read @0 (value :UInt64) -> (value :UInt64);
}
interface Node extends(Base) $Cxx.allowCancellation {
  hold @0 (value :UInt64) -> (value :UInt64);
}
