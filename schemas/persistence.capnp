@0xb9f1d6444d5da36d;
using Cxx = import "/capnp/c++.capnp";
$Cxx.allowCancellation;

# Realm-specific parameters for the standard capnp.Persistent interface.
# Owner IDs are stable across key rotation. Only authenticated transport
# identities, supplied by the host bootstrap factory, can restore a reference.
struct Owner { id @0 :Data; } # Exactly 16 nonzero bytes.
struct SturdyRef {
  realm @0 :Data; # Exactly 16 bytes.
  token @1 :Data; # Exactly 32 opaque bytes, including 192 random bits in v2; do not log.
}
interface Restorer {
  restore @0 (reference :SturdyRef) -> (cap :Capability);
}
