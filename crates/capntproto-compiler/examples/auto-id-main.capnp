using I = import "auto-id-types.capnp";
struct Config { # A configuration schema without a persistent file ID.
  server @0 :I.Server;
  details :group { enabled @1 :Bool = true; }
  union { count @2 :UInt32; label @3 :Text; }
  struct Child { value @0 :UInt16 = 17; }
}
struct Fixed @0xeddddddddddddddd { struct Child {} }
interface Service { call @0 (arg :Config) -> (result :I.Server); }
const defaults :Config = (details = (enabled = false), label = "auto");
