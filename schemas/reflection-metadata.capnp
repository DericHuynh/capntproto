@0xc9dd18dcb8d1d4f1;

annotation note(*) :Text;
annotation rank(*) :UInt32;

struct Box(T) {
  value @0 :T;
  annotation label(*) :T;
}

enum Color $note("palette") {
  red @0 $note("warm");
  blue @1 $rank(7);
}

struct Record $note("record") $Box(Text).label("generic") {
  name @0 :Text $note("field");
  count @1 :UInt32 $rank(9);
  color @2 :Color;
}

interface Service $note("interface") {
  call @0 (value :Record) -> (value :Record) $note("method");
}

struct Destination $Box(Record).label((name = "annotated", count = 6)) {
  record @0 :Record;
  records @1 :List(Record);
  color @2 :Color;
  boxed @3 :Box(Text);
  service @4 :Service;
  text @5 :Text;
}

const answer :UInt32 = 42;
const greeting :Text = "hello";
const bytes :Data = 0x"00ff10";
const color :Color = blue;
const record :Record = (name = "Ada", count = 3, color = red);
const records :List(Record) = [(name = "one"), (name = "two", count = 2)];
const words :List(Text) = ["first", "second"];
const boxed :Box(Text) = (value = "inside");
const nothing :Void = void;
const yes :Bool = true;
const i8 :Int8 = -8;
const i16 :Int16 = -160;
const i32 :Int32 = -320;
const i64 :Int64 = -640;
const u8 :UInt8 = 8;
const u16 :UInt16 = 160;
const u64 :UInt64 = 640;
const f32 :Float32 = 1.5;
const f64 :Float64 = -2.25;
