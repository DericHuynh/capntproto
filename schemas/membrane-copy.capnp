@0x9e6a958b3408f769;
using Cxx = import "/capnp/c++.capnp";
$Cxx.namespace("membraneCopy");
$Cxx.allowCancellation;
interface Service {
  ping @0 () -> (value :UInt32);
  bounce @1 (cap :Service) -> (cap :Service);
}
struct Payload {
  number @0 :UInt32;
  cap @1 :Service;
  other @2 :Service;
  caps @3 :List(Service);
  nested @4 :List(Payload);
  opaque @5 :AnyPointer;
}
struct Empty {}
struct Grouped {
  outside @0 :Service;
  body :group {
    count @1 :UInt32;
    cap @2 :Service;
    nested :group { other @3 :Service; }
  }
}
