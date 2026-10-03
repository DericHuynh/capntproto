@0xeb1638529467ca0f;

annotation label(interface, method, param) :Text;

struct Payload {
  text @0 :Text = "default";
  count @1 :UInt32 = 7;
}

interface Base {
  ping @0 (value :UInt32 = 42) -> (value :UInt32);
}

interface Service extends(Base) $label("example service") {
  echo @0 Payload -> Payload $label("echo");
  getPeer @1 () -> (peer :Base $label("capability"));
  upload @2 (bytes :Data) -> stream;
}
