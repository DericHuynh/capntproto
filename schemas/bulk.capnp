@0xded2de8526cbe5c9;
using Cxx = import "/capnp/c++.capnp";
$Cxx.allowCancellation;
# One capability authorizes one transfer. Chunks are ordered, nonempty data;
# write replies acknowledge remote processing. done publishes the complete value.
struct Config {
  length @0 :UInt64;
  maxChunkBytes @1 :UInt32;
  windowBytes @2 :UInt32;
  maxChunks @3 :UInt32;
}
struct Summary { bytes @0 :UInt64; chunks @1 :UInt64; }
enum Status { receiving @0; complete @1; canceled @2; failed @3; }
interface Transfer {
  describe @0 () -> (config :Config);
  write @1 (sequence :UInt64, data :Data) -> (sequence :UInt64);
  done @2 () -> (summary :Summary);
  cancel @3 () -> (status :Status);
}
# Durable variant: identical retries preserve the committed prefix; describe
# provides a checkpoint after reconnect. The host reissues this capability only
# through its authenticated application authority boundary.
interface DurableTransfer {
  describe @0 () -> (config :Config, progress :Summary, status :Status,
                     revision :UInt64, sha256 :Data);
  write @1 (sequence :UInt64, data :Data) -> (sequence :UInt64);
  done @2 () -> (summary :Summary, revision :UInt64);
  cancel @3 () -> (status :Status);
}
