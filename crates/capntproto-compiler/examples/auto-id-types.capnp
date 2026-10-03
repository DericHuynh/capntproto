using Root = import "auto-id-main.capnp";
struct Server {
  owner @0 :Root.Config;
  port @1 :UInt16 = 8080;
}
