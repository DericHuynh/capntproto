------------------------- MODULE RpcRevokedPolicy -------------------------
EXTENDS Naturals, TLC
CONSTANTS RedirectAfterRevoke, SkipPolicy, LeakTarget
VARIABLES revoked, called, fresh, result, policyCalls, targetBroken, calledRevoked
vars == <<revoked,called,fresh,result,policyCalls,targetBroken,calledRevoked>>
Init == /\ revoked = FALSE /\ called = FALSE /\ fresh = FALSE /\ result = 0
        /\ policyCalls = 0 /\ targetBroken = FALSE /\ calledRevoked = FALSE
Revoke == /\ ~revoked /\ revoked' = TRUE
          /\ UNCHANGED <<called,fresh,result,policyCalls,targetBroken,calledRevoked>>
Fresh == /\ revoked /\ ~called /\ ~fresh /\ fresh' = TRUE
         /\ UNCHANGED <<revoked,called,result,policyCalls,targetBroken,calledRevoked>>
Call == /\ ~called /\ called' = TRUE /\ calledRevoked' = revoked
        /\ policyCalls' = IF revoked /\ SkipPolicy THEN 0 ELSE 1
        /\ targetBroken' = (revoked /\ ~LeakTarget)
        /\ result' = IF ~revoked \/ LeakTarget THEN 1 ELSE
                      IF RedirectAfterRevoke /\ ~SkipPolicy THEN 2 ELSE 3
        /\ UNCHANGED <<revoked,fresh>>
Next == Revoke \/ Fresh \/ Call
Spec == Init /\ [][Next]_vars
TypeOK == /\ revoked \in BOOLEAN /\ called \in BOOLEAN /\ fresh \in BOOLEAN
          /\ result \in 0..3 /\ policyCalls \in 0..1 /\ targetBroken \in BOOLEAN /\ calledRevoked \in BOOLEAN
PolicyAlwaysConsulted == called => policyCalls = 1
RevokedTargetIsBroken == called => targetBroken = calledRevoked
NoRevokedAuthority == calledRevoked => result # 1
RedirectProvenance == result = 2 => (calledRevoked /\ RedirectAfterRevoke)
=============================================================================
