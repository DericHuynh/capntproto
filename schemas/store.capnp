@0xccc65f78c7249241;
using Cxx = import "/capnp/c++.capnp";
$Cxx.allowCancellation;
# Whole-entry revisions. put stages a durable private revision; publish atomically
# changes the visible revision if the expected published revision still matches.
interface Object(T) {
  get @0 () -> (value :T, revision :UInt64);
  put @1 (expectedHead :UInt64, value :T) -> (revision :UInt64);
  publish @2 (revision :UInt64, expectedPublished :UInt64) -> (revision :UInt64);
  subscribe @3 (after :UInt64, observer :Observer(T)) -> (subscription :Subscription);
  # Pull every retained publication. The caller persists its last processed
  # revision and supplies it to next(); opening a stream never advances it.
  history @4 () -> (history :History(T), floor :UInt64, published :UInt64);
}
interface History(T) {
  next @0 (after :UInt64) -> (result :HistoryResult(T));
  cancel @1 () -> ();
}
struct HistoryResult(T) {
  # Cursors below this floor have lost history. Zero denotes complete history.
  floor @0 :UInt64;
  union {
    event :group { value @1 :T; revision @2 :UInt64; }
    gap @3 :Void;
  }
}
interface Observer(T) {
  changed @0 (value :T, revision :UInt64) -> ();
}
interface Subscription { cancel @0 () -> (); }
struct Document { text @0 :Text; }
# A facet bound to one component ID and schema. Revisions/CAS belong to the
# entire object. put stages a new root; commit publishes the entire new root,
# including other components inherited from its head. No raw byte patching.
interface Component(T) {
  get @0 () -> (value :T, revision :UInt64);
  put @1 (expectedHead :UInt64, value :T) -> (revision :UInt64);
  commit @2 (expectedHead :UInt64, expectedPublished :UInt64, value :T)
      -> (revision :UInt64);
}
# Acceptance is served only on the Native session bound to this introduction.
interface Handoff(T) {
  accept @0 (id :Data) -> (object :Object(T));
}
struct IntroductionTicket {
  id @0 :Data;
  target @1 :Data;
  recipient @2 :Data;
  psk @3 :Data;
  context @4 :Data;
  address @5 :Text;
}
# The owner offers a direct connection with the configured delegated facet.
interface Introducer(T) {
  provide @0 (recipient :Data) -> (ticket :IntroductionTicket);
}
interface IntroductionReceiver(T) {
  introduce @0 (ticket :IntroductionTicket) -> ();
}
