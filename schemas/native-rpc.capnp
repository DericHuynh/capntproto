@0xc512d3b8d33ca578;
using Cxx = import "/capnp/c++.capnp";
$Cxx.namespace("nativeRpc");
$Cxx.allowCancellation;
interface Service(T) { ping @0 (value :UInt32) -> (value :UInt32); }
interface Derived(T) extends(Service(T)) {}
interface Other { ping @0 () -> (); }
struct Envelope(T) { body :group { cap @0 :T; } }
struct Outer(T) { padding @0 :Data; inner @1 :Envelope(T); }
struct Wrong { number @0 :UInt32; }
interface Factory { open @0 () -> Outer(Service(Text)); }
