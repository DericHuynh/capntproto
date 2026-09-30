@0xea1029b843d7fc65;

annotation label @0xf1a0235c49bd786e (*) :Text;
annotation marker @0xc735a02e1498bf6d (file, field, group, union, annotation) :Void;
annotation rule(struct) :Rule $marker;

$label("annotation example");
$marker;

struct Rule {
  limit @0 :UInt32 = 8;
  tags @1 :List(Text);
}

struct Message $label("first") $label("second")
               $rule(limit = 12, tags = ["fast", "portable"]) {
  payload @0 :Data $marker $label("wire bytes");
  metadata :group $marker {
    enabled @1 :Bool = true $label("flag");
  }
  choice :union $marker {
    empty @2 :Void;
    value @3 :Text $label("selected");
  }
}

enum State $label("states") {
  idle @0 $label("waiting");
  ready @1 $label("running");
}

const bytes :Data = 0x"00ff" $label("signature");
