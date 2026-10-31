@0xd1e9d884696c006f;
interface Service {
  read @0 (value :UInt64, bytes :UInt32) -> (value :UInt64, payload :Data);
  child @1 () -> (service :Service);
  callback @2 (service :Service) -> (value :UInt64);
}
