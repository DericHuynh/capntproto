@0xa9f1b6781367a89b;

struct Scope(A, B) {
  first @0 :A;
  second @1 :B;
  struct Inner(C) {
    outer @0 :A;
    local @1 :C;
  }
  struct Unused { count @0 :UInt32; }
}

interface Service(T) { call @0 (value :T) -> (value :T); }

struct Choice(T) {
  serial @0 :UInt32;
  union {
    empty @1 :Void;
    value @2 :T;
    part :group { text @3 :Text; count @4 :UInt32; }
  }
  trailer @5 :Text;
  named :union { left @6 :Text; right @7 :Text; }
}

struct Cases {
  nested @0 :Scope(Text, Data).Inner(List(Text));
  defaults @1 :Scope(AnyPointer, AnyPointer).Inner(AnyPointer);
  unused @2 :Scope(Text, Data).Unused;
  choice @3 :Choice(Text);
  cap @4 :Service(Text);
}
