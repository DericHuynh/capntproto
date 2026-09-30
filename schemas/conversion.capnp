@0xe774d7d0d6808f1c;
using Cxx = import "/capnp/c++.capnp";
$Cxx.namespace("conversionTest");

struct Target {
  union {
    kept @0 :Capability;
    empty @1 :Void;
    flag @2 :Bool;
    int8 @3 :Int8;
    int16 @4 :Int16;
    int32 @5 :Int32;
    int64 @6 :Int64;
    uint8 @7 :UInt8;
    uint16 @8 :UInt16;
    uint32 @9 :UInt32;
    uint64 @10 :UInt64;
    float32 @11 :Float32;
    float64 @12 :Float64;
    kind @13 :Kind;
    text @14 :Text;
    data @15 :Data;
  }
}
enum Kind { zero @0; other @1; }
enum Foreign { zero @0; other @1; }
