@0xdb94a2765cd97104;
using Cxx = import "/capnp/c++.capnp";
$Cxx.namespace("enumBrand");
struct Scope(T) {
  annotation note(enum, enumerant) :Text;
  enum Tone $note("tone") { quiet @0 $note("quiet"); loud @1; }
  tone @0 :Tone;
  tones @1 :List(Tone);
  payload @2 :T;
  struct Inner(U) {
    annotation label(enum, enumerant) :Text;
    enum State $label("state") { idle @0; busy @1 $label("busy"); }
    state @0 :State;
    outer @1 :Tone;
    value @2 :U;
  }
}
struct Record {
  text @0 :Scope(Text);
  data @1 :Scope(Data);
  union {
    other @2 :UInt32;
    tone @3 :Scope(Text).Tone;
  }
}
