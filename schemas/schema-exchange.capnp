@0xee9c804328a392d1;
using Cxx = import "/capnp/c++.capnp";
$Cxx.allowCancellation;
# Application-level schema exchange. Revisions are immutable catalog versions,
# distinct from Cap'n Proto type IDs. All dependencies use the same revision.
struct Key { id @0 :UInt64; revision @1 :UInt64; }
struct Definition {
  key @0 :Key;
  # Exactly one unpacked message rooted at /capnp/schema.capnp Node.
  node @1 :Data;
}
interface Catalog {
  get @0 (key :Key) -> (result :Result);
  struct Result {
    union { missing @0 :Void; found @1 :Definition; }
  }
}
