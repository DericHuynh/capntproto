@0xcf6934aa93d6edc1;
using Cxx = import "/capnp/c++.capnp";
$Cxx.allowCancellation;

interface Base {
  struct Value { value @0 :UInt32; }
  echo @0 (value :UInt32) -> Value;
}

interface Service extends(Base) {
  # A named result makes forwarding compatibility explicit across methods.
  struct Opened { cap @0 :Base; }
  open @0 () -> Opened;
  forward @1 (cap :Base, value :UInt32) -> Base.Value;
  forwardCap @2 (cap :Service) -> Opened;
  stream @3 () -> stream;
}
