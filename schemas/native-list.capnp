@0xcf951ee7d72d3a89;
using Cxx = import "/capnp/c++.capnp";
$Cxx.namespace("nativeList");
enum Choice { zero @0; one @1; }
struct Item(T) { number @0 :UInt32; payload @1 :T; }
interface Service { ping @0 (value :UInt32) -> (value :UInt32); }
interface Other { ping @0 () -> (); }
interface Derived extends(Service) {}
struct Lists {
  numbers @0 :List(UInt32);
  choices @1 :List(Choice);
  records @2 :List(Item(Text));
  nested @3 :List(List(Item(Text)));
  caps @4 :List(Service);
  deepNumbers @5 :List(List(UInt32));
  bytes @6 :List(UInt8);
  texts @7 :List(Text);
  blobs @8 :List(Data);
  flags @9 :List(Bool);
  derived @10 :List(Derived);
}

# Nested mutable-list fixtures also exercise schema evolution via raw pointers.
struct NestedLists {
  numbers @0 :List(List(UInt32));
  flags @1 :List(List(Bool));
  choices @2 :List(List(Choice));
  records @3 :List(List(Item(Text)));
  texts @4 :List(List(Text));
  blobs @5 :List(List(Data));
  caps @6 :List(List(Service));
  voids @7 :List(List(Void));
  triples @8 :List(List(List(UInt32)));
}

struct CastItem(T) { number @0 :UInt32 = 17; payload @1 :T; }
struct CastTypes {
  text @0 :CastItem(Text);
  data @1 :CastItem(Data);
  cap @2 :CastItem(Service);
  records @3 :List(CastItem(Text));
  numbers @4 :List(UInt32);
  choices @5 :List(Choice);
  texts @6 :List(Text);
  blobs @7 :List(Data);
  caps @8 :List(Service);
  nested @9 :List(List(UInt32));
  flags @10 :List(Bool);
}
