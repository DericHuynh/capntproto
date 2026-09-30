# Leading comments are ordinary comments, not file documentation.
@0xbbaadeadbeef1213;
# Documentation for this file.

struct Record {
  # Documentation for Record, attached after its opening brace.
  later @1 :Text;
  # Documentation follows a field, independently of ordinal order.
  first @0 :UInt32 = 7; # First field.
  metadata :group { # Group documentation belongs to the node and field.
    count @2 :UInt32;
  } # The opening comment takes precedence over this closing comment.
  union {
    # An unnamed union has no separate source-info entry.
    none @3 :Void;
    text @4 :Text;
  }
  struct Nested {} # Closing comments document a block without an opening comment.
}

enum State {
  # State documentation.
  ready @1; # Ready.
  unknown @0; # Unknown.
}

interface Service {
  # Service documentation.
  get @0 (key :Text, # Parameter comments are ordinary comments.
          limit :UInt32 = 7) -> (value :Record); # Method documentation.
  ping @1 (); # The implicit result has no source location.
}

const label :Text = "# not a comment"; # Label documentation.
annotation note(*) :Text; # Annotation documentation.
