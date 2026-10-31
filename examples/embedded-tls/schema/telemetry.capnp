@0xa3146b40a5dce977;
# Bounded telemetry messages over TLS; no hosted capability RPC allocator.
struct Reading { deviceId @0 :UInt64; sequence @1 :UInt64; milliCelsius @2 :Int32; }
struct Acknowledgement { sequence @0 :UInt64; accepted @1 :Bool; }
