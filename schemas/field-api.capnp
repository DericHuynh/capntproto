@0xc1b49f1aad77cb26;
using Rust = import "/rust.capnp";
# Reader/editor facade acceptance schema.
struct Person {
  id @0 :UInt64 = 42;
  name @1 :Text = "anonymous";
  address @2 :Address = (city = "default city");
  previousAddress @3 :Address;
  phones @4 :List(PhoneNumber);
  payload @5 :Data;
  names @6 :List(Text);
  numbers @7 :List(UInt32);
  kinds @8 :List(PhoneKind);
  optionalName @9 :Text $Rust.option;
  employment :union {
    unemployed @10 :Void;
    employer @11 :Text;
    school @12 :Text;
    selfEmployed @13 :Void;
  }
  matrix @14 :List(List(UInt32));
  enabled @15 :Bool = true;
  ratio @16 :Float64 = -1.5;
}
struct Address { city @0 :Text; }
struct PhoneNumber { number @0 :Text; kind @1 :PhoneKind; }
enum PhoneKind { mobile @0; home @1; work @2; }
struct Box(T) { value @0 :T; }
struct Types {
  box @0 :Box(Text);
  any @1 :AnyPointer;
  cap @2 :Service;
  signed @3 :Int64 = -25;
  boxedPeople @4 :Box(List(Person));
  nested @5 :Scope(Person).Inner(Text);
  service @6 :GenericService(Person);
}
interface Service {
  echo @0 (person :Person) -> (person :Person);
  make @1 () -> (cap :Service);
  stream @2 (person :Person) -> stream;
}
struct OldRecord { value @0 :UInt64; }
struct NewRecord { value @0 :UInt64; extra @1 :Text; }
struct Evolving { record @0 :NewRecord; records @1 :List(NewRecord); }
struct Choice {
  untouched @0 :UInt64;
  union {
    empty @1 :Void;
    text @2 :Text;
    details :group {
      count @3 :UInt64;
      label @4 :Text;
      nested :union { none @5 :Void; value @6 :Text; }
    }
  }
}

struct Scope(T) { struct Inner(U) { outer @0 :T; inner @1 :U; } }
interface GenericService(T) { call @0 (value :T) -> (value :T); }

struct RenamedFields {
  read @0 :Text $Rust.name("label");
  type @1 :Text;
  choice @2 :RenamedEnum;
}
enum RenamedEnum { original @0 $Rust.name("renamed"); self @1; unknown @2; }
struct Transfer {
  source @0 :Types;
  destination @1 :Types;
}
struct Historical { records @0 :List(OldRecord); }
struct Future { records @0 :List(NewRecord); }
struct CapForest { items @0 :List(List(Service)); }
struct Opaque {}
struct OpaqueTransfer { source @0 :Opaque; destination @1 :Opaque; }
struct PrimitiveHistory { records @0 :List(UInt64); }
struct GenericHistory { records @0 :Box(List(OldRecord)); }
struct GenericPrimitiveHistory { records @0 :Box(List(UInt64)); }
struct Recursive {
  value @0 :UInt64;
  next @1 :Recursive;
  children @2 :List(Recursive);
  voids @3 :List(Void);
}
struct NativeRecord { cap @0 :Service; label @1 :Text; }

# Staged group publication must preserve siblings, including packed data bits.
struct StagedChoice(T) {
  siblingBit @0 :Bool = true;
  siblingByte @1 :UInt8 = 211;
  siblingText @2 :Text;
  siblingCap @3 :Service;
  union {
    empty @4 :Void;
    old @5 :Service;
    details :group {
      flag @6 :Bool = true;
      count @7 :UInt64 = 42;
      label @8 :Text = "default label";
      value @9 :T;
      nested :union {
        none @10 :Void;
        first :group { cap @11 :Service; small @12 :UInt16 = 9; }
        second :group { cap @13 :Service; number @14 :Float64 = -1.5; }
      }
    }
  }
}
struct ScalarGroup {
  sibling @0 :Bool = true;
  union { empty @1 :Void; details :group { flag @2 :Bool = true; } }
}
