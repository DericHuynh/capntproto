@0x9dba12d9d7a1c4e0;
# Imported declarations.
struct Box(T) {
  # Generic box.
  value @0 :T;
  struct Unused { value @0 :UInt64; }
}
struct Entry {
  # An entry.
  value @0 :Int16 = -7;
}
struct Unused { value @0 :Bool; }
