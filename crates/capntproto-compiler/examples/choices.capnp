@0xa9bc8f0ad2e61937;

struct Message {
  sequence @0 :UInt64;
  metadata :group {
    enabled @1 :Bool = true;
    title @2 :Text = "default";
  }
  union {
    empty @3 :Void;
    reading :group {
      value @4 :Float64;
      unit @5 :Text;
    }
    error :group {
      code @6 :UInt32;
      text @7 :Text;
    }
    nested :union {
      none @8 :Void;
      some @9 :UInt64;
    }
  }
}
