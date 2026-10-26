@0xf57e16659c5682dd;

# Rust generator regression: all enclosing parameters must remain bound even
# when a union group's fields do not use them. Keep this separate from the
# C++ interoperability schema: the pinned C++ generator omits a required
# `template` disambiguator for this nested generic group.
struct GroupScope(A) {
  struct Inner(B, C) {
    union {
      payload :group { count @0 :UInt32; }
      empty @1 :Void;
    }
  }
}
