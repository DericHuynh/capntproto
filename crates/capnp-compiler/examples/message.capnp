@0xdba1c3d4e5f60718;

# A standalone schema compiled without the C++ frontend.
struct Message {
  sequence @0 :UInt64 = 42;
  title @1 :Text = "hello\nworld";
  state @2 :State = ready;
  samples @3 :List(Int32);
  child @4 :Child;
  flag @5 :Bool = true;
  blob @6 :Data;
  opaque @7 :AnyPointer;
  nested @8 :List(List(Text));
  enum State {
    unknown @0;
    ready @1;
  }
  struct Child {
    parent @0 :Message;
    state @1 :State;
  }
}

struct Scalars @0xef0123456789abcd {
  nothing @0 :Void = void;
  i8 @1 :Int8 = -128;
  i16 @2 :Int16 = -32768;
  i32 @3 :Int32 = -2147483648;
  i64 @4 :Int64 = -9223372036854775808;
  u8 @5 :UInt8 = 255;
  u16 @6 :UInt16 = 65535;
  u32 @7 :UInt32 = 4294967295;
  u64 @8 :UInt64 = 0xffffffffffffffff;
  f32 @9 :Float32 = 1.25e2;
  f64 @10 :Float64 = -2.5e-20;
  state @11 :Message.State;
}
