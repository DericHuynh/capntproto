@0xdac45738125fbe90;

struct Envelope(T) {
  value @0 :T;
  metadata :group {
    backup @1 :T;
    state @2 :State = ready;
  }
  enum State { empty @0; ready @1; }
}

struct Defaults {
  message @0 :Envelope(Text) = (value = "generic default");
}

struct Outer(A) {
  struct Pair(B, C) { first @0 :B; second @1 :C; }
  interface Service(B, C) {
    exchange @0 Pair(C, A) -> Pair(B, A);
  }
}

interface Echo(T) {
  echo @0 (value :T) -> (value :T);
}

interface Service(T) extends(Echo(T)) {
  wrap @0 (value :T) -> (value :Envelope(T));
  relay @1 [U] (value :U = null) -> (value :U);
}
