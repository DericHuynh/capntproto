@0xfedaabcdabcdffff;
# FILE-DOC: **Schema documentation** with Unicode 🦀.
#
# See https://example.com/schema and (https://example.com/nested_(value)).
# Existing [links](https://example.com/existing) and `http://code.example` keep their form.
#
# Untagged examples describe schema syntax:
#
#     value @0 :Text;
#
# - A nested example:
#
#   ```
#   this is not Rust
#   ```
#
# > A quoted example:
# >
# >     also not Rust;
#
# `````text
# literal ```` fence
# `````

using Rust = import "/rust.capnp";
using Dep = import "dep.capnp";

struct SchemaDocumentation {} # COLLISION-DOC: A real schema declaration.

struct Record $Rust.name("RenamedRecord") {
  # RECORD-DOC: Document the renamed record.
  #
  # Explicit Rust examples remain doctests:
  #
  # ```rust
  # assert_eq!(2 + 2, 4);
  # ```
  later @1 :Text $Rust.name("renamedLater"); # LATER-DOC: Text in ordinal one.
  first @0 :UInt32 = 7; # FIRST-DOC: Scalar in ordinal zero.
  metadata :group { # GROUP-DOC: Metadata group.
    count @2 :UInt32; # COUNT-DOC: Nested count.
  }
  union {
    none @3 :Void; # NONE-DOC: Empty union arm.
    text @4 :Text; # TEXT-DOC: Text union arm.
  }
  imported @5 :Dep.Item; # IMPORT-DOC: Imported item.
  struct Nested {} # NESTED-DOC: A nested declaration.
}

struct Box(T) { # GENERIC-DOC: A generic record.
  value @0 :T; # VALUE-DOC: The generic field.
}

enum State { # STATE-DOC: Ordinal order differs from source order.
  ready @1 $Rust.name("Available"); # READY-DOC: Renamed variant.
  unknown @0; # UNKNOWN-DOC: First variant.
}

interface Base { # BASE-DOC: Base service.
  ping @0 (); # PING-DOC: Inherited method.
}
interface Service extends(Base) { # SERVICE-DOC: Service interface.
  get @0 (key :Text) -> (value :Record); # GET-DOC: Fetch a record.
  send @1 (value :Record) -> stream; # SEND-DOC: Streaming method.
}

const label :Text = "value"; # LABEL-DOC: Text constant.
const labels :List(Text) = ["one", "two"]; # LABELS-DOC: Pointer constant.
annotation note(*) :Text;
# NOTE-DOC: Annotation declaration. Quotes and comment terminators stay text:
# "]
# compile_error!("a schema comment became code");
# /* */ \ 🦀
