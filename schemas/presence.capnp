@0xce922bf2dddf9a12;
using Cxx = import "/capnp/c++.capnp";
$Cxx.namespace("presenceTest");

struct Sample {
  empty @0 :Void;
  flag @1 :Bool = true;
  int8 @2 :Int8 = -7;
  int16 @3 :Int16 = -1234;
  int32 @4 :Int32 = -1234567;
  int64 @5 :Int64 = -9223372036854775808;
  uint8 @6 :UInt8 = 241;
  uint16 @7 :UInt16 = 65535;
  uint32 @8 :UInt32 = 42;
  uint64 @9 :UInt64 = 18446744073709551615;
  zero32 @10 :Float32 = -0.0;
  zero64 @11 :Float64 = -0.0;
  nan32 @12 :Float32 = nan;
  nan64 @13 :Float64 = nan;
  kind @14 :Kind = other;
  text @15 :Text = "default";
  data @16 :Data = 0x"cafe";
  list @17 :List(UInt16) = [1, 2];
  child @18 :Child = (count = 5);
  any @19 :AnyPointer;
  cap @20 :Capability;
  body :group {
    count @21 :UInt32 = 42;
    label @22 :Text = "default";
    zero @23 :Float32 = -0.0;
    nan @24 :Float64 = nan;
    kind @25 :Kind = other;
    union {
      none @26 :Void;
      number @27 :UInt32 = 42;
      note @28 :Text = "default";
      details :group { flag @29 :Bool = true; }
    }
  }
  enum Kind { zero @0; other @1; }
}
struct Child { count @0 :UInt32; }

struct GroupReset {
  marker @0 :UInt32 = 99;
  body :group {
    count @1 :UInt32 = 7;
    text @2 :Text = "body";
    union { none @3 :Void; wide @4 :UInt64; pointer @5 :Text; }
    nested :group {
      union {
        defaults :group { small @6 :UInt8 = 5; flag @7 :Bool = true; }
        other @8 :UInt64;
        object @9 :Text;
      }
      note @10 :Text = "nested";
    }
  }
  union {
    idle @11 :Void;
    picked :group {
      value @12 :UInt32 = 42;
      note @13 :Text = "picked";
      inner :group { flag @14 :Bool = true; }
    }
  }
  branded @15 :GenericGroup(Text);
}
struct GenericGroup(T) {
  body :group { value @0 :T; count @1 :UInt32 = 42; }
}

struct MutableAccess {
  marker @0 :UInt32;
  union {
    none @1 :Void;
    child @2 :AccessPayload = (value = 17, text = "child-default");
    nums @3 :List(UInt16) = [3, 5];
    children @4 :List(AccessPayload) = [(value = 9, text = "list-default")];
    group :group { count @5 :UInt32 = 42; item @6 :AccessPayload; }
    branded @7 :GenericGroup(Text);
    nested @8 :List(List(UInt16)) = [[7, 8]];
    caps @9 :List(AccessCapability);
    cap @10 :Capability;
  }
  outside @11 :AccessPayload = (value = 71, text = "outside-default");
  plainNums @12 :List(UInt16) = [11, 13];
}
struct AccessPayload {
  value @0 :UInt32 = 7;
  text @1 :Text = "payload";
  cap @2 :Capability;
}
interface AccessCapability {}

struct BlobAccess {
  union {
    none @0 :Void;
    text @1 :Text = "default";
    data @2 :Data = 0x"cafe";
    cap @3 :Capability;
  }
  plainText @4 :Text;
  plainData @5 :Data;
  texts @6 :List(Text);
  datas @7 :List(Data);
  numbers @8 :List(UInt32);
  brandedText @9 :GenericGroup(Text);
  brandedData @10 :GenericGroup(Data);
  marker @11 :UInt32 = 42;
  child @12 :AccessPayload;
}

struct PointerAccess {
  marker @0 :UInt32;
  union {
    none @1 :Void;
    any @2 :AnyPointer;
    structure @3 :AnyStruct;
    list @4 :AnyList;
    cap @5 :Capability;
    typed @6 :AccessCapability;
    text @7 :Text;
  }
  plainAny @8 :AnyPointer;
  plainStruct @9 :AnyStruct;
  plainList @10 :AnyList;
  boundAny @11 :GenericGroup(AnyPointer);
  details :group { structure @12 :AnyStruct; list @13 :AnyList; }
  boundText @14 :GenericGroup(Text);
}
