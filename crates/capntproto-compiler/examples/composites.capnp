@0xe2395a0f41cb92d7;

enum State { idle @0; ready @1; }

struct Item {
  count @0 :UInt32 = 42;
  name @1 :Text = "default";
  state @2 :State = ready;
  payloads @3 :List(Data) = [0x"00ff", "text"];
  metadata :group {
    enabled @4 :Bool = true;
    union {
      absent @5 :Void;
      label @6 :Text;
    }
  }
  children @7 :List(Item);
}

const empty :Item = ();
const item :Item = (
  count = 7, name = "hé🦀", state = idle,
  metadata = (enabled = false, label = "selected"),
  children = [(), (count = 9)],
);
const items :List(Item) = [.empty, .item];
const rows :List(List(Int16)) = [[-1, 2], [], [32767]];
const erased :AnyPointer = .item;

struct Defaults {
  item @0 :Item = .item;
  items @1 :List(Item) = .items;
  rows @2 :List(List(Int16)) = .rows;
  erased @3 :AnyPointer = .item;
  erasedRows @4 :AnyPointer = .rows;
}
