--------------------- MODULE RpcIntroducedBootstrap ---------------------
EXTENDS Naturals, TLC
CONSTANTS CachedPeer, ReissueDelegation
VARIABLES delegated, cToB, bToC
vars == <<delegated, cToB, bToC>>
\* Initially A holds capabilities from B and C, each issued for authenticated
\* peer A. Grant values encode host and grantee (C-for-A = 310).
\* Introduce is the completed standard handoff, checked at finer granularity
\* separately by RpcWireHandoff. It creates B's connection to C.
Init == /\ delegated = 0 /\ cToB = 0 /\ bToC = 0
Introduce == /\ delegated = 0
             /\ delegated' = IF ReissueDelegation THEN 320 ELSE 310
             /\ UNCHANGED <<cToB, bToC>>
BootstrapCtoB == /\ delegated # 0 /\ cToB = 0
                 /\ cToB' = IF CachedPeer THEN 210 ELSE 230
                 /\ UNCHANGED <<delegated, bToC>>
BootstrapBtoC == /\ delegated # 0 /\ bToC = 0
                 /\ bToC' = IF CachedPeer THEN 310 ELSE 320
                 /\ UNCHANGED <<delegated, cToB>>
Next == Introduce \/ BootstrapCtoB \/ BootstrapBtoC
Spec == Init /\ [][Next]_vars
TypeOK == /\ delegated \in {0,310,320} /\ cToB \in {0,210,230} /\ bToC \in {0,310,320}
DelegationPreservesAuthority == delegated \in {0,310}
AuthenticatedPeer == /\ cToB \in {0,230} /\ bToC \in {0,320}
=============================================================================
