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

# Separate, versioned topology suite; existing workflow contracts stay unchanged.
struct AccessNode {
  value @0 :UInt64;
  payload @1 :Data;
  next @2 :AccessNode;
}
interface AccessService {
  access @0 (sequence :UInt64, depth :UInt16, root :AccessNode)
      -> (checksum :UInt64, visits :UInt32, root :AccessNode);
  describe @1 () -> (hops :UInt16);
}
