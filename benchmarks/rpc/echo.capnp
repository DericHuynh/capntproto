@0x83d4a973dcdf17d1;
interface Echo {
  echo @0 (sequence :UInt64, payload :Data) -> (sequence :UInt64, payload :Data);
}

# Stateful and capability-oriented workflows shared by Rust and upstream C++.
interface Workflows {
  echo @0 (sequence :UInt64, payload :Data) -> (sequence :UInt64, payload :Data);
  open @1 (payload :Data) -> (cell :Cell);
  callback @2 (sink :Callback, sequence :UInt64, payload :Data) -> (sequence :UInt64);
}
interface Cell {
  read @0 () -> (version :UInt64, payload :Data);
  write @1 (expectedVersion :UInt64, payload :Data) -> (version :UInt64);
}
interface Callback {
  notify @0 (sequence :UInt64, payload :Data) -> (sequence :UInt64);
}
