----------------------- MODULE RpcMembraneTransform -----------------------
EXTENDS Naturals, TLC
CONSTANTS Custom, Reverse, UnwrapCustom, RevokeCustom
VARIABLES crossed, returned, revoked, used, escaped, result, expected
vars == <<crossed,returned,revoked,used,escaped,result,expected>>
Init == /\ crossed = FALSE /\ returned = FALSE /\ revoked = FALSE /\ used = FALSE
        /\ escaped = FALSE /\ result = 0 /\ expected = 0
Cross == /\ ~crossed /\ crossed' = TRUE
         /\ UNCHANGED <<returned,revoked,used,escaped,result,expected>>
Return == /\ crossed /\ ~returned /\ ~used /\ returned' = TRUE
          /\ escaped' = (~Custom /\ ~revoked)
          /\ UNCHANGED <<crossed,revoked,used,result,expected>>
Revoke == /\ ~revoked /\ revoked' = TRUE
          /\ UNCHANGED <<crossed,returned,used,escaped,result,expected>>
Wanted == IF Custom THEN (IF Reverse = returned THEN 2 ELSE 3)
          ELSE IF revoked /\ ~escaped THEN 9 ELSE 1
Use == /\ crossed /\ ~used /\ used' = TRUE /\ expected' = Wanted
       /\ result' = IF Custom /\ returned /\ UnwrapCustom THEN 1 ELSE
                     IF Custom /\ revoked /\ RevokeCustom THEN 9 ELSE Wanted
       /\ UNCHANGED <<crossed,returned,revoked,escaped>>
Next == Cross \/ Return \/ Revoke \/ Use
Spec == Init /\ [][Next]_vars
TypeOK == /\ crossed \in BOOLEAN /\ returned \in BOOLEAN /\ revoked \in BOOLEAN /\ used \in BOOLEAN
          /\ escaped \in BOOLEAN /\ result \in {0,1,2,3,9} /\ expected \in {0,1,2,3,9}
AuthorityProvenance == result = expected
ReversibleDefault == escaped => (returned /\ ~Custom)
=============================================================================
