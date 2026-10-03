@0xbabd19620168fc40;
# Declarations discovered before they are compiled.
struct Used { value @0 :UInt32 = 11; }
struct Box(T) {
  # Compiled on demand.
  value @0 :T;
  struct First { flag @0 :Bool; }
  struct Second { text @0 :Text; }
  using Alias = First;
  using Parameter = T;
}
struct Later {
  # A lazy import and an embed.
  value @0 :import "session-late.capnp".Item;
  label @1 :Text = embed "reflection.txt";
  box @2 :Box(Text);
  info :group { active @3 :Bool = true; }
}
interface Service(T) {
  call @0 (item :Box(T)) -> (result :T);
}
const limit :UInt32 = 42;
annotation note(*) :Text;
struct Broken { value @0 :DoesNotExist; }
using Alias = Later;
using TextBox = Box(Text);
using Remote = import "session-late.capnp";
using limitAlias = limit;
using noteAlias = note;
