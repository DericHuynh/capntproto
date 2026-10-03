@0xbbedfa70ed497843;
using Cxx = import "/capnp/c++.capnp";
$Cxx.allowCancellation;

# A delegated authority for one host, recipient, advertised endpoint and context.
# Carry this capability on an already-established confidential control route.
interface Provisioner {
  # Optional recipient mapping from the same socket used for the Native dial.
  # Only rendezvous-enabled providers accept it; it grants no peer identity.
  reserve @0 (rendezvousAddress :Text) -> (ticket :Ticket, lease :Lease);
}
struct Ticket {
  host @0 :Data;
  recipient @1 :Data;
  address @2 :Text;
  connectionId @3 :Data;
  psk @4 :Data;
  context @5 :Data;
}
interface Lease {
  # Resolves only after the host authenticates and installs its native RPC route.
  # On arbitrated networks, this acknowledges admission to the pending endpoint;
  # session selection must still complete before native RPC data is published.
  # Dropping a waiter does not revoke other holders of this lease.
  ready @0 () -> ();
  # Cancels a pending handshake; cannot undo a route already being installed.
  cancel @1 () -> ();
}
