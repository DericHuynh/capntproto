@0xafedbc9876543210;
using Main = import "main.capnp";

enum State {
  unknown @0;
  ready @1;
}

struct Record {
  id @0 :UInt64;
  state @1 :State;
  owner @2 :Main.Message;
}
