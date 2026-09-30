@0xc180a52d61537931;
using Cxx = import "/capnp/c++.capnp";
$Cxx.allowCancellation;
using Provisioning = import "noise-provisioning.capnp";

# Recipient-scoped read authority. Publication authority stays with the owner.
# Host keys authenticate sessions; names are resolved by this trusted directory.
interface Directory {
  lookup @0 (host :Data) -> (record :Record);
  resolve @1 (name :Text) -> (record :Record);
}
struct Record {
  host @0 :Data;
  recipient @1 :Data;
  address @2 :Text;
  context @3 :Data;
  generation @4 :UInt64;
  remainingMillis @5 :UInt32;
  provider @6 :Provisioning.Provisioner;
}
