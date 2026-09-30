@0xb21c2b90b4f4b6a9;
using Cxx = import "/capnp/c++.capnp";
$Cxx.allowCancellation;
$Cxx.namespace("dynamicTest");
using Harness = import "runtime-test.capnp".Harness;
interface Base(T) {
  transform @0 (value :UInt32, cap :T) -> (value :UInt32, cap :T);
  stream @1 (value :UInt32) -> stream;
  pending @2 (cap :T) -> (cap :T);
}
interface Derived(T) extends(Base(T)) {
  own @0 () -> (cap :T);
}
interface Other { other @0 () -> (); }
struct Envelope {
  cap @0 :Derived(Harness);
  caps @1 :List(Derived(Harness));
  any @2 :AnyPointer;
  body :group { cap @3 :Derived(Harness); }
}
struct Parcel(T) { value @0 :T; }
interface Marker(T) { ping @0 () -> (value :UInt32); }
interface Wrap(T) extends(Base(Parcel(T))) {}
struct Scope(T) {
  struct Inner(U) { left @0 :T; right @1 :U; }
}
struct BrandEnvelope {
  union { sentinel @0 :UInt32; cap @1 :Base(Text); }
  item @2 :Parcel(Text);
  items @3 :List(Parcel(Text));
  caps @4 :List(Base(Text));
}
struct CompoundEnvelope {
  item @0 :Parcel(Base(Text));
  items @1 :List(Parcel(Base(Text)));
}

struct OrphanPayload { cap @0 :Harness; text @1 :Text; number @2 :UInt32 = 42; }
struct OrphanGroup {
  sibling @0 :UInt64;
  body :group {
    cap @1 :Harness;
    label @2 :Text;
    union {
      empty @3 :Void;
      value @4 :UInt32;
      nested :group { flag @5 :Bool; text @6 :Text; cap @7 :Harness; }
    }
  }
}
struct OrphanCase {
  source @0 :OrphanPayload;
  target @1 :OrphanPayload;
  union { sentinel @2 :UInt32 = 17; arm @3 :OrphanPayload; }
  values @4 :List(OrphanPayload);
  texts @5 :List(Text);
  numbers @6 :List(UInt32);
  flag @7 :Bool = true;
  float @8 :Float64 = -1.5;
  token @9 :Harness;
  tokens @10 :List(Harness);
  any @11 :AnyPointer;
  description @12 :Text = "default";
  kind @13 :Kind = other;
  kinds @14 :List(Kind);
  groups @15 :List(OrphanGroup);
  enum Kind { zero @0; other @1; }
}
struct SmallOrphan { number @0 :UInt32; }
struct ExternalCase { data @0 :Data; other @1 :Data; token @2 :Harness; any @3 :AnyPointer; }
struct SmallOrphanCase { source @0 :SmallOrphan; targets @1 :List(SmallOrphan); }
struct LargeOrphanCase { source @0 :OrphanPayload; targets @1 :List(OrphanPayload); }
struct OrphanBrands {
  text @0 :Parcel(Text);
  data @1 :Parcel(Data);
  derived @2 :Derived(Text);
  base @3 :Base(Text);
  wrong @4 :Base(Data);
}

struct OrphanPointerKinds {
  body :group {
    structure @0 :AnyStruct;
    list @1 :AnyList;
    cap @2 :Capability;
    typed @3 :Harness;
  }
}

struct GroupDefaults(T) {
  sibling @0 :UInt64;
  body :group {
    label @1 :Text = "default";
    count @2 :UInt32 = 42;
    item @3 :T;
    union {
      nested :group { text @4 :Text = "nested"; flag @5 :Bool = true; }
      note @6 :Text = "note";
    }
  }
}

struct GroupScope(A) {
  struct Inner(B, C) {
    union {
      payload :group { count @0 :UInt32; }
      empty @1 :Void;
    }
  }
}
