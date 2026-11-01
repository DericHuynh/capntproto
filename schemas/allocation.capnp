@0xd6d47fe4272b482e;

struct Record {
  count @0 :UInt32 = 42;
  label @1 :Text = "unassigned";
  numbers @2 :List(UInt32);
  children @3 :List(Child);
  nested @4 :Box(Text);
  payload @5 :Data;
  union {
    idle @6 :Void;
    details :group {
      enabled @7 :Bool;
      value @8 :UInt64;
    }
  }
}
struct Child { id @0 :UInt64; }
struct Empty {}
struct Box(T) { value @0 :T; }
struct ChoiceBox(T) { union { idle @0 :Void; value @1 :T; count @2 :UInt32; } }

enum State { waiting @0; ready @1; }
struct Entry { id @0 :UInt64; label @1 :Text; }
struct EntryV2 { id @0 :UInt64; label @1 :Text; extra @2 :UInt64 = 17; payload @3 :Data; }
struct EntryWide { id @0 :UInt64; label @1 :Text; extra @2 :UInt64 = 17; counter @3 :UInt64; }
struct Collections {
  bits @0 :List(Bool);
  numbers @1 :List(UInt64);
  names @2 :List(Text);
  chunks @3 :List(Data);
  entries @4 :List(Entry);
  matrix @5 :List(List(UInt16));
  states @6 :List(State);
  sibling @7 :UInt32;
  plain :group { value @8 :UInt16 = 9; label @9 :Text; }
  union {
    idle @10 :Void;
    selected @11 :List(Entry);
    details :group { value @12 :UInt64; label @13 :Text; }
  }
}

struct BoundedRecord {
  id @0 :UInt32 = 42;
  title @1 :Text = "default";
  payload @2 :Data;
  child @3 :Child;
  union {
    idle @4 :Void;
    note @5 :Text;
    number @6 :UInt32;
    linked @7 :Child;
  }
}

interface Echo {
  echo @0 (value :UInt64) -> (value :UInt64);
  capability @1 (value :Echo) -> (value :Echo);
  list @2 (values :List(Echo)) -> (values :List(Echo));
  write @3 (value :UInt64) -> stream;
  nested @4 () -> (value :PipelineNode);
}
struct PipelineNode {
  echo @0 :Echo;
  child @1 :PipelineNode;
  group :group { echo @2 :Echo; }
  union { idle @3 :Void; selected @4 :Echo; }
}
interface GenericService(T) {
  echo @0 (value :T) -> (value :T);
}
interface DerivedService extends(Echo) {
  call @0 GenericParams -> GenericResults;
}
struct GenericParams { value @0 :GenericService(Text); }
struct GenericResults { value @0 :GenericService(Text); }

interface TextService extends(GenericService(Text)) {}
