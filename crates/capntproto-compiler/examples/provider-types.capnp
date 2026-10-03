@0xbbbbbbbbbbbbbbbb;
using M = import "root:alias";
struct Item {
  root @0 :M.Root;
  value @1 :UInt32 = 29;
}
struct Later { text @0 :Text = "loaded later"; }
using Unused = import "never:open";
