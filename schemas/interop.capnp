@0xe973f1a2175aed83;
using Cxx = import "/capnp/c++.capnp";
$Cxx.allowCancellation;

# Compiled independently by our compiler, upstream capnpc, and pinned C++.
interface Peer {
  struct Echo { value @0 :UInt64; payload @1 :Data; text @2 :Text; }
  echo @0 (value :UInt64, payload :Data, text :Text) -> Echo;
  bounce @1 (cap :Peer) -> (cap :Peer);
  child @2 () -> (cap :Peer);
  tail @3 (cap :Peer, value :UInt64) -> Echo;
  callback @4 (cap :Peer, value :UInt64) -> Echo;
  fail @5 (kind :UInt16) -> ();
  hold @6 () -> ();
  stats @7 () -> (started :UInt32, canceled :UInt32, released :UInt32, streamed :UInt64);
  promise @8 () -> (cap :Peer);
  resolve @9 (reject :Bool) -> ();
  stream @10 (value :UInt64) -> stream;
  order @11 (value :UInt32) -> ();
}
