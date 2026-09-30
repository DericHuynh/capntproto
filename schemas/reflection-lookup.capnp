@0xe716ce8c975d221a;
using Cxx = import "/capnp/c++.capnp";
$Cxx.namespace("reflectionLookup");
enum Tone { zulu @0; alpha @1; middle @2; }
struct Scope(T) {
  struct Nested { value @0 :T; }
}
interface Root(T) { match @0 (value :T) -> (value :T); }
interface Left(T) extends(Root(T)) {}
interface Right(T) { match @0 (value :T) -> (value :T); }
interface Diamond(T) extends(Left(T), Right(T)) { own @0 () -> (); }
interface Override(T) extends(Diamond(T)) { match @0 (value :T) -> (value :T); }
interface Empty {}
interface Wide1 {}
interface Wide2 {}
interface Wide3 extends(Wide1, Wide2) {}
interface Wide4 extends(Wide1, Wide2) {}
interface Wide5 extends(Wide3, Wide4) {}
interface Wide6 extends(Wide3, Wide4) {}
interface Wide7 extends(Wide5, Wide6) {}
interface Wide8 extends(Wide5, Wide6) {}
interface Wide9 extends(Wide7, Wide8) {}
interface Wide10 extends(Wide7, Wide8) {}
interface Limit63 extends(Wide9, Wide10) {}
interface Limit64 extends(Limit63) {}
interface Limit65 extends(Limit64) {}
struct Cache(T) { first @0 :UInt32; second @1 :T; }
interface Implicit { call @0 [T] (value :T) -> (value :T); }
