@0xb9fa19d25ac43e76;
using Json = import "/capnp/compat/json.capnp";
struct Record {
  flag @0 :Bool;
  i8 @1 :Int8;
  i16 @2 :Int16;
  i32 @3 :Int32;
  i64 @4 :Int64;
  u8 @5 :UInt8;
  u16 @6 :UInt16;
  u32 @7 :UInt32;
  u64 @8 :UInt64;
  f32 @9 :Float32;
  f64 @10 :Float64;
  text @11 :Text = "default";
  data @12 :Data;
  nums @13 :List(Int64);
  child @14 :Record;
  children @15 :List(Record);
  lists @16 :List(List(UInt16));
  choice @17 :Choice;
  union { none @18 :Void; some @19 :Text; }
  details :group { amount @20 :UInt32; }
}
enum Choice { first @0; second @1 $Json.name("SECOND"); }
struct Annotated $Json.discriminator(name = "kind", valueName = "value") {
  renamed @0 :Text $Json.name("renamed_field");
  details :group $Json.flatten(prefix = "p_") {
    count @1 :UInt32;
    more :group $Json.flatten() { flag @2 :Bool; }
  }
  data @3 :Data $Json.base64;
  hex @4 :Data $Json.hex;
  choices @5 :List(Choice);
  embedded @6 :Json.Value;
  union { none @7 :Void; number @8 :Int32; }
}
struct FlatUnion $Json.discriminator(name = "type") {
  union {
    a :group $Json.flatten() { x @0 :UInt32; }
    b @1 :Record $Json.flatten(prefix = "b_");
  }
}
interface Calculator {
  add @0 (x :Int32, y :Int32) -> (value :Int32) $Json.name("sum");
  notify @1 (value :Int32) -> () $Json.notification;
  fail @2 () -> ();
}
struct Box(T) { value @0 :T; }
struct Branded { text @0 :Box(Text); data @1 :Box(Data); }

struct BoolBox { value @0 :Bool = true; spare @1 :Text = "kept default"; }
# Declare the second field first: wrapping follows schema order, not source order.
struct NumberBox { spare @1 :Text = "kept default"; value @0 :Float32; }
struct IntegerBox { value @0 :Int8; }
struct VoidBox { value @0 :Void; spare @1 :UInt32 = 42; }
enum LiteralNames { first @0; true @1; false @2; void @3; inf @4; nan @5; }
struct EnumBox { value @0 :LiteralNames; }
struct EmptyBox {}
struct UnionBox { union { value @0 :UInt32 = 7; other @1 :Text; } spare @2 :UInt32 = 42; }
struct GroupBox { first :group { value @0 :Bool; } }
struct Wrapping {
  flag @0 :BoolBox;
  number @1 :NumberBox;
  integer @2 :IntegerBox;
  emptyValue @3 :VoidBox;
  text @4 :Box(Text);
  data @5 :Box(Data);
  choice @6 :EnumBox;
  nested @7 :Box(Box(Text));
  empty @8 :EmptyBox;
  list @9 :Box(List(UInt32));
  unionBox @10 :UnionBox;
  groupBox @11 :GroupBox;
  flags @12 :List(BoolBox);
  texts @13 :List(Box(Text));
  numbers @14 :List(NumberBox);
  details :group { value @15 :UInt32 = 7; spare @16 :Text = "group default"; }
  union {
    none @17 :Void;
    picked @18 :BoolBox;
    group :group { value @19 :Bool; spare @20 :Text = "union default"; }
  }
}

# Exercise resetting nested groups without erasing inactive union storage.
struct AssignmentGroups {
  marker @0 :UInt32 = 99;
  box :group {
    union { none @1 :Void; wide @2 :UInt64; pointer @3 :Text; }
    extra @4 :UInt32 = 7;
    nested :group {
      union { defaults :group { value @5 :UInt8 = 5; } other @6 :UInt64; }
      note @7 :Text = "nested";
    }
  }
  union {
    idle @8 :Void;
    picked :group { value @9 :UInt32 = 42; note @10 :Text = "chosen"; }
  }
}
