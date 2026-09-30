@0xcedcf8c8157f910e;
interface Echo {
  echo @0 (value :UInt64) -> (value :UInt64);
  child @1 () -> (cap :Echo);
}
