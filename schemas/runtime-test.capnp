@0xebf0c12a3ac9c94e;
using Cxx = import "/capnp/c++.capnp";
$Cxx.allowCancellation;
interface Harness {
  struct Value { value @0 :UInt32; }
  echo @0 (value :UInt32) -> Value;
  bounce @1 (cap :Harness) -> (cap :Harness);
  tail @2 (cap :Harness, value :UInt32) -> Value;
  pending @3 (holdContext :Bool) -> (cap :Harness);
  tailCap @4 (cap :Harness) -> (cap :Harness);
  tailRoundtrip @5 (cap :Harness, value :UInt32) -> Value;
  fdCaps @6 (caps :List(Harness)) -> (caps :List(Harness));
  struct PlainCycle { next @0 :PlainCycle; }
  struct CapCycle { next @0 :CapCycle; cap @1 :Harness; }
  struct Grouped { body :group { cap @0 :Harness; } }
  struct Box(T) { value @0 :T; }
  plainCycle @7 () -> PlainCycle;
  capCycle @8 () -> CapCycle;
  nestedLists @9 () -> (caps :List(List(Harness)));
  grouped @10 () -> Grouped;
  opaque @11 () -> (value :AnyPointer);
  generic @12 () -> Box(Data);
  stream @13 () -> stream;
}

# Explicit method structs deliberately reorder, repeat and omit generic
# arguments. Rust aliases must still declare parameters in lexical order.
struct GenericOuter(A) {
  struct Pair(X, Y) { first @0 :X; second @1 :Y; }
  interface Service(B, C) {
    exchange @0 Pair(C, A) -> Pair(B, A);
    repeat @1 Pair(C, C) -> Pair(A, A);
  }
}
