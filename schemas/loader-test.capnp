@0xbee76348ee52a9d7;
struct Pair(A, B) { first @0 :A; second @1 :B; }
interface Factory { exchange @0 [T, U] (first :T, second :U) -> Pair(U, T); }
