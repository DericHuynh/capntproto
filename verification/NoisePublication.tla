-------------------------- MODULE NoisePublication --------------------------
EXTENDS Naturals
CONSTANT Fault
VARIABLES generation, owned, active, expired, closed, stopped, replaced, attempts,
          renewed, accepted, saved, event
vars == <<generation,owned,active,expired,closed,stopped,replaced,attempts,
          renewed,accepted,saved,event>>
Init == /\ generation=1 /\ owned=1 /\ active=1 /\ expired=0 /\ closed=0
        /\ stopped=0 /\ replaced=0 /\ attempts=0 /\ renewed=0 /\ accepted=0
        /\ saved=0 /\ event=0
Eligible == active=1 /\ expired=0 /\ closed=0 /\ stopped=0 /\ generation=owned
Renew == /\ attempts<2 /\ event'=1 /\ attempts'=attempts+1
         /\ saved'=IF Eligible THEN 1 ELSE 0
         /\ LET allowed == Eligible \/ (Fault="expiredRenew" /\ expired=1 /\ active=1
                             /\ closed=0 /\ stopped=0 /\ generation=owned)
                           \/ (Fault="replaceRenew" /\ replaced=1 /\ active=1 /\ closed=0 /\ stopped=0)
            IN /\ accepted'=IF allowed THEN 1 ELSE 0
               /\ generation'=IF allowed THEN generation+1 ELSE generation
               /\ owned'=IF allowed THEN generation+1 ELSE owned
               /\ renewed'=IF allowed THEN renewed+1 ELSE renewed
               /\ expired'=IF allowed THEN 0 ELSE expired
         /\ UNCHANGED <<active,closed,stopped,replaced>>
Replace == /\ active=1 /\ closed=0 /\ replaced=0 /\ generation'=generation+1
           /\ expired'=0 /\ replaced'=1 /\ event'=2
           /\ UNCHANGED <<owned,active,closed,stopped,attempts,renewed,accepted,saved>>
Expire == /\ active=1 /\ expired=0 /\ expired'=1 /\ event'=3
          /\ UNCHANGED <<generation,owned,active,closed,stopped,replaced,attempts,renewed,accepted,saved>>
Revoke == /\ active=1 /\ active'=0 /\ event'=4
          /\ UNCHANGED <<generation,owned,expired,closed,stopped,replaced,attempts,renewed,accepted,saved>>
Close == /\ closed=0 /\ closed'=1 /\ active'=0 /\ event'=5
         /\ UNCHANGED <<generation,owned,expired,stopped,replaced,attempts,renewed,accepted,saved>>
Stop == /\ stopped=0 /\ stopped'=1 /\ event'=6 /\ saved'=active
        /\ active'=IF generation=owned \/ Fault="staleDrop" THEN 0 ELSE active
        /\ UNCHANGED <<generation,owned,expired,closed,replaced,attempts,renewed,accepted>>
Next == Renew \/ Replace \/ Expire \/ Revoke \/ Close \/ Stop
Spec == Init /\ [][Next]_vars
TypeOK == /\ generation\in 1..4 /\ owned\in 1..4 /\ attempts\in 0..2 /\ renewed\in 0..2
          /\ active\in 0..1 /\ expired\in 0..1 /\ closed\in 0..1 /\ stopped\in 0..1
          /\ replaced\in 0..1 /\ accepted\in 0..1 /\ saved\in 0..1 /\ event\in 0..6
RenewalAuthority == event=1 => accepted=saved
SuccessorSurvives == event=6 /\ generation#owned => active=saved
Closed == closed=1 => active=0
LiveSpec == Spec /\ WF_vars(Stop)
OwnerStops == stopped=0 ~> stopped=1
=============================================================================
