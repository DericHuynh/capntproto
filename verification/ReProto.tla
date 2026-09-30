----------------------------- MODULE ReProto -----------------------------
EXTENDS Naturals, TLC
CONSTANTS Bound, BrokenEmbargo
VARIABLES head, published, phase, pending, drained, direct, accepted, revoked
vars == <<head, published, phase, pending, drained, direct, accepted, revoked>>
Init == /\ head = 0 /\ published = 0 /\ phase = 0 /\ pending = 0
        /\ drained = 0 /\ direct = 0 /\ accepted = 0 /\ revoked = FALSE
Stage == /\ ~revoked /\ head < Bound /\ head' = head + 1
         /\ UNCHANGED <<published, phase, pending, drained, direct, accepted, revoked>>
Publish == /\ ~revoked /\ \E r \in (published+1)..head: published' = r
           /\ UNCHANGED <<head, phase, pending, drained, direct, accepted, revoked>>
Enqueue == /\ ~revoked /\ phase = 0 /\ pending + drained < Bound
           /\ pending' = pending + 1
           /\ UNCHANGED <<head, published, phase, drained, direct, accepted, revoked>>
Drain == /\ pending > 0 /\ pending' = pending - 1 /\ drained' = drained + 1
         /\ UNCHANGED <<head, published, phase, direct, accepted, revoked>>
Provide == /\ ~revoked /\ phase = 0 /\ phase' = 1
           /\ UNCHANGED <<head, published, pending, drained, direct, accepted, revoked>>
Accept == /\ ~revoked /\ phase = 1 /\ phase' = 2 /\ accepted' = 1
          /\ UNCHANGED <<head, published, pending, drained, direct, revoked>>
ReplayAccept == /\ ~revoked /\ phase >= 2 /\ UNCHANGED vars
Lift == /\ ~revoked /\ phase = 2 /\ (pending = 0 \/ BrokenEmbargo)
        /\ phase' = 3
        /\ UNCHANGED <<head, published, pending, drained, direct, accepted, revoked>>
Direct == /\ ~revoked /\ phase = 3 /\ direct < Bound /\ direct' = direct + 1
          /\ UNCHANGED <<head, published, phase, pending, drained, accepted, revoked>>
Revoke == /\ ~revoked /\ revoked' = TRUE
          /\ UNCHANGED <<head, published, phase, pending, drained, direct, accepted>>
Next == Stage \/ Publish \/ Enqueue \/ Drain \/ Provide \/ Accept \/ ReplayAccept \/ Lift \/ Direct \/ Revoke
Spec == Init /\ [][Next]_vars /\ WF_vars(Drain)
TypeOK == /\ head \in 0..Bound /\ published \in 0..head
          /\ phase \in 0..3 /\ pending \in 0..Bound /\ drained \in 0..Bound
          /\ pending + drained <= Bound /\ direct \in 0..Bound
          /\ accepted \in 0..1 /\ revoked \in BOOLEAN
Embargo == phase = 3 => pending = 0
SingleAcceptance == (phase >= 2) <=> (accepted = 1)
DirectNeedsAcceptance == direct > 0 => phase = 3 /\ accepted = 1
RevocationStable == [][revoked => revoked']_vars
NoEffectsAfterRevoke == [][revoked => UNCHANGED <<head, published, direct, accepted>>]_vars
Drains == (pending > 0) ~> (pending = 0)
=============================================================================
