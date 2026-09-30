---------------------------- MODULE RpcMembrane ----------------------------
EXTENDS Naturals, TLC
CONSTANTS Reflect, EarlyRedirect, WrongRoot, ForgetResolution
VARIABLES called, resolved, revoked, result, observed
vars == <<called,resolved,revoked,result,observed>>
\* An exported promise resolves either internally or to an external capability
\* imported through a related policy. Policies request resolve-before-redirect.
\* Result 0 pending, 1 outside target, 2 policy redirect, 3 revoked.
Dispatch == IF observed /\ Reflect /\ ~ForgetResolution THEN 1 ELSE IF revoked THEN 3 ELSE IF ~resolved THEN (IF EarlyRedirect THEN 2 ELSE 0)
            ELSE IF Reflect /\ ~WrongRoot THEN 1 ELSE 2
Init == /\ called = FALSE /\ resolved = FALSE /\ revoked = FALSE /\ result = 0 /\ observed = FALSE
Call == /\ ~called /\ called' = TRUE /\ result' = Dispatch /\ UNCHANGED <<resolved,revoked,observed>>
Resolve == /\ ~resolved /\ resolved' = TRUE
           /\ result' = IF called /\ result = 0 THEN (IF Reflect /\ ~WrongRoot THEN 1 ELSE 2) ELSE result
           /\ UNCHANGED <<called,revoked,observed>>
Revoke == /\ ~revoked /\ revoked' = TRUE
          /\ result' = IF called /\ result = 0 THEN 3 ELSE result
          /\ UNCHANGED <<called,resolved,observed>>
Observe == /\ resolved /\ ~revoked /\ ~called /\ ~observed /\ observed' = TRUE
           /\ UNCHANGED <<called,resolved,revoked,result>>
Next == Call \/ Resolve \/ Revoke \/ Observe
Spec == Init /\ [][Next]_vars
TypeOK == /\ called \in BOOLEAN /\ resolved \in BOOLEAN /\ revoked \in BOOLEAN /\ result \in 0..3 /\ observed \in BOOLEAN
ResultProvenance == result # 0 => called
ResolveBeforeRedirect == result = 2 => resolved /\ ~Reflect
ReflectedCapability == result = 1 => resolved /\ Reflect
RevocationCompletesPending == called /\ revoked => result # 0
CachedReflection == called /\ observed /\ Reflect => result = 1
=============================================================================
