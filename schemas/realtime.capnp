@0x91e8e9a415c6bb72;
using Cxx = import "/capnp/c++.capnp";
$Cxx.allowCancellation;
enum Outcome { applied @0; expired @1; superseded @2; busy @3; canceled @4; closed @5; }
struct Config {
  clockDomain @0 :Text;
  clockSkew @1 :UInt64;
  keys @2 :UInt32;
  capacity @3 :UInt32;
  maxSequence @4 :UInt64;
  maxPayloadBytes @5 :UInt32;
  maxWaiters @6 :UInt32;
}
# A fresh capability defines a fresh sequence namespace. Payloads are complete
# replaceable snapshots, not dependent deltas or arbitrary side-effecting calls.
interface Snapshots {
  describe @0 () -> (config :Config);
  offer @1 (sequence :UInt64, key :UInt32, notAfter :UInt64, snapshot :Data) -> (outcome :Outcome);
  cancel @2 (sequence :UInt64) -> (outcome :Outcome);
  close @3 () -> ();
}

# Session-local bearer authority for the unreliable snapshot lane. Export only
# over a confidential, authorized RPC route. A token is useless on other sessions.
struct DatagramStatus {
  union {
    unknown @0 :Void;
    pending @1 :Void;
    terminal @2 :Outcome;
  }
}
interface DatagramSnapshots {
  describe @0 () -> (config :Config, token :Data);
  status @1 (sequence :UInt64) -> (status :DatagramStatus, closed :Bool);
  cancel @2 (sequence :UInt64) -> (outcome :Outcome);
  close @3 () -> ();
}
