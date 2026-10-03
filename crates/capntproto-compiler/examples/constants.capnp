@0xdf047a68b19023ce;

using Count = UInt32;
const limit :Count = 64;
const label :Text = "Rust " "frontend";
const signature :Data = 0x"00 ff 7e 80";
const fraction :Float32 = 0.1;
const preferred :State = ready;

enum State {
  unknown @0;
  ready @1;
}

struct Settings {
  const maxCount @0xd45c17e9ac3042b8 :UInt64 = .limit;
  count @0 :UInt64 = Settings.maxCount;
  name @1 :Text = .label;
  signature @2 :Data = .signature;
  fraction @3 :Float64 = .fraction;
  state @4 :State = ready;
}
