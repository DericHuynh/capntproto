@0xaaaaaaaaaaaaaaaa;
using A = import "pkg:types";
using B = import "/alias//../types";
struct Root { # Served by application callbacks.
  a @0 :A.Item;
  b @1 :B.Item;
  payload @2 :Data = embed "asset:blob";
}
const blob :Data = embed "asset:blob";
