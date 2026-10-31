@0xbac48cc0bca70bc4;
# Public shopping API. A Customer capability binds authority to one account.
enum Status { processing @0; cancelling @1; confirmed @2; cancelled @3; declined @4; }
struct Snapshot {
  key @0 :Text;
  quantity @1 :UInt32;
  cents @2 :UInt64;
  status @3 :Status;
  revision @4 :UInt64;
}
interface Shop {
  login @0 (account :Text, token :Text) -> (customer :Customer);
  catalog @1 () -> (unitCents :UInt64, bulkMinimum :UInt32, bulkUnitCents :UInt64);
}
interface Customer {
  place @0 (key :Text, quantity :UInt32) -> (order :Order, snapshot :Snapshot);
  cancel @1 (key :Text) -> (snapshot :Snapshot);
  view @2 (key :Text) -> (snapshot :Snapshot);
  list @3 () -> (orders :List(Snapshot));
}
interface Order {
  state @0 () -> (snapshot :Snapshot);
  settled @1 () -> (snapshot :Snapshot);
  cancel @2 () -> (snapshot :Snapshot);
}
# Internal service authority is protected by mutual TLS, not exposed by Shop.
enum ResourceState { held @0; committed @1; released @2; rejected @3; }
struct Claim {
  key @0 :Text;
  bucket @1 :Text;
  units @2 :UInt64;
  fingerprint @3 :Text;
}
struct Entry { claim @0 :Claim; state @1 :ResourceState; }
struct Bucket { name @0 :Text; initial @1 :UInt64; available @2 :UInt64; }
interface Resource {
  reserve @0 (claim :Claim) -> (state :ResourceState);
  commit @1 (claim :Claim) -> (state :ResourceState);
  release @2 (claim :Claim) -> (state :ResourceState);
  audit @3 () -> (buckets :List(Bucket), entries :List(Entry));
}
