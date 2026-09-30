@0xbc77bbaadc125124;
using Dep = import "leaf.capnp";

const names :List(Text) = ["zero", "one"];
const blobs :List(Data) = [0x"00ff", 0x"abcd"];
const sample :Record = (names = ["constant"], leaf = (label = "imported"));

struct Record {
  names @0 :List(Text) = ["default"];
  blobs @1 :List(Data) = [0x"00ff"];
  nestedNames @2 :List(List(Text));
  nestedBlobs @3 :List(List(Data));
  leaf @4 :Dep.Leaf;
  leaves @5 :List(Dep.Leaf);
  state @6 :Dep.State = ready;
  states @7 :List(Dep.State);
  boxed @8 :Box(List(Text));
  union {
    empty @9 :Void;
    details :group { label @10 :Text; bytes @11 :Data; }
  }
}

struct Handles {
  service @0 :Service;
  services @1 :List(Service);
  generic @2 :Generic(Dep.Leaf);
}

struct Box(T) { value @0 :T; }
interface Base { ping @0 () -> (); }
interface Service extends(Base) {
  echo @0 (names :List(Text), blobs :List(Data)) -> (record :Record);
  send @1 (record :Record) -> stream;
}
interface Generic(T) { echo @0 (value :T) -> (value :T); }
