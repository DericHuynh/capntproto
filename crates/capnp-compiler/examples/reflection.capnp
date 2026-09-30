@0xbdd19cb358d9e6a1;
# Runtime reflection example.
using Common = import "reflection-common.capnp";
using Alias = Record;

annotation note(*) :Text;
const limit :UInt32 = 17;
# Default count.

struct Record $note("root") {
  # Record documentation 🦀.
  enum Status {
    ready @0; # Ready.
    done @1; # Done.
  }
  struct Child { label @0 :Text; }
  using ChildAlias = Child;
  count @0 :UInt32 = .limit; # Number of records.
  label @1 :Text = embed "reflection.txt";
  box @2 :Common.Box(Text);
  entries @3 :List(Common.Entry);
  details :group {
    # Inline details.
    active @4 :Bool = true; # Enabled.
  }
  union {
    empty @5 :Void;
    child @6 :Child;
  }
  status @7 :Status = ready;
}

interface Service {
  # Service documentation.
  fetch @0 (key :Text) -> (record :Record); # Fetch a record.
}
