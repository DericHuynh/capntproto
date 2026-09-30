--------------------------- MODULE RpcBootstrap ---------------------------
EXTENDS Naturals, TLC
CONSTANTS WrongPeer, StalePolicy
VARIABLES epoch, a, b, aFirst, aLast, bFirst, bLast, usedA, usedB,
          aEpoch, bEpoch, obsolete
vars == <<epoch, a, b, aFirst, aLast, bFirst, bLast, usedA, usedB,
          aEpoch, bEpoch, obsolete>>
\* Example application policy, not a protocol-mandated access control rule:
\* peer 2 loses permission for new bootstraps at epoch 1; peer 3 gets a new
\* capability. A policy change does not revoke previously issued capabilities.
\* Each request includes an immediate pipelined capability call. Wire replay
\* checks both Return payloads; these are drained at each abstract boundary.
Grant(peer, version) == IF peer = 2 /\ version = 1 THEN 1 ELSE peer * 10 + version
Actual(peer) == Grant(IF WrongPeer THEN 3 ELSE peer, IF StalePolicy THEN 0 ELSE epoch)
Init == /\ epoch = 0 /\ a = 0 /\ b = 0 /\ aFirst = 0 /\ aLast = 0
        /\ bFirst = 0 /\ bLast = 0 /\ usedA = FALSE /\ usedB = FALSE
        /\ aEpoch = 0 /\ bEpoch = 0 /\ obsolete = FALSE
Policy == /\ epoch = 0 /\ epoch' = 1
          /\ UNCHANGED <<a, b, aFirst, aLast, bFirst, bLast, usedA, usedB, aEpoch, bEpoch, obsolete>>
RequestA == /\ a < 2 /\ a' = a + 1 /\ aLast' = Actual(2) /\ aEpoch' = epoch
            /\ aFirst' = IF a = 0 THEN aLast' ELSE aFirst
            /\ UNCHANGED <<epoch, b, bFirst, bLast, usedA, usedB, bEpoch, obsolete>>
RequestB == /\ b < 2 /\ b' = b + 1 /\ bLast' = Actual(3) /\ bEpoch' = epoch
            /\ bFirst' = IF b = 0 THEN bLast' ELSE bFirst
            /\ UNCHANGED <<epoch, a, aFirst, aLast, usedA, usedB, aEpoch, obsolete>>
UseA == /\ aFirst >= 20 /\ ~usedA /\ usedA' = TRUE
        /\ UNCHANGED <<epoch, a, b, aFirst, aLast, bFirst, bLast, usedB, aEpoch, bEpoch, obsolete>>
UseB == /\ bFirst >= 30 /\ ~usedB /\ usedB' = TRUE
        /\ UNCHANGED <<epoch, a, b, aFirst, aLast, bFirst, bLast, usedA, aEpoch, bEpoch, obsolete>>
Obsolete == /\ ~obsolete /\ obsolete' = TRUE
            /\ UNCHANGED <<epoch, a, b, aFirst, aLast, bFirst, bLast, usedA, usedB, aEpoch, bEpoch>>
Next == Policy \/ RequestA \/ RequestB \/ UseA \/ UseB \/ Obsolete
Spec == Init /\ [][Next]_vars
TypeOK == /\ epoch \in 0..1 /\ a \in 0..2 /\ b \in 0..2
          /\ aFirst \in {0,1,20,21,30,31} /\ aLast \in {0,1,20,21,30,31}
          /\ bFirst \in {0,1,30,31} /\ bLast \in {0,1,30,31}
          /\ usedA \in BOOLEAN /\ usedB \in BOOLEAN /\ obsolete \in BOOLEAN
          /\ aEpoch \in 0..1 /\ bEpoch \in 0..1
PeerIsolation == /\ aFirst \in {0,1,20,21} /\ aLast \in {0,1,20,21}
                 /\ bFirst \in {0,30,31} /\ bLast \in {0,30,31}
FreshPolicy == /\ (a > 0 => aLast = Grant(2,aEpoch)) /\ (b > 0 => bLast = Grant(3,bEpoch))
CapabilityAuthority == /\ (usedA => aFirst = 20) /\ (usedB => bFirst \in {30,31})
=============================================================================
