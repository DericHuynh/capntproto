using Shared = import "common.capnp";
@0xeadfc123456789ab;
using Shared.Record;

struct Message {
  using State = Shared.State;
  record @0 :Record;
  batch @1 :List(Shared.Record);
  state @2 :State = ready;
}
