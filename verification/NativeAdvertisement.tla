------------------------ MODULE NativeAdvertisement ------------------------
EXTENDS Naturals
CONSTANT Fault
VARIABLES phase, current, address, provider, generation, owned, moved, lost,
          recovered, retired, dropped, oldLive, event, saved
vars == <<phase,current,address,provider,generation,owned,moved,lost,recovered,
          retired,dropped,oldLive,event,saved>>
\* phase: 0 published, 1 suspended, 2 stopped. current: 0 absent, 1 owner, 2 successor.
Init == /\ phase=0 /\ current=1 /\ address=1 /\ provider=1 /\ generation=1 /\ owned=1
        /\ moved=0 /\ lost=0 /\ recovered=0 /\ retired=0 /\ dropped=0 /\ oldLive=1
        /\ event=0 /\ saved=0
Move == /\ phase=0 /\ moved=0 /\ address=1 /\ moved'=1 /\ address'=2
        /\ provider'=IF Fault="splitAddress" THEN 1 ELSE 2
        /\ generation'=generation+1 /\ owned'=generation+1 /\ oldLive'=0 /\ event'=1
        /\ UNCHANGED <<phase,current,lost,recovered,retired,dropped,saved>>
Lose == /\ phase=0 /\ lost=0 /\ lost'=1 /\ phase'=1 /\ address'=0 /\ provider'=0
        /\ generation'=generation+1 /\ owned'=generation+1
        /\ oldLive'=(IF Fault="staleProvider" THEN 1 ELSE 0) /\ event'=2
        /\ UNCHANGED <<current,moved,recovered,retired,dropped,saved>>
Recover == /\ lost=1 /\ recovered=0 /\ recovered'=1 /\ event'=3
           /\ LET allowed == phase=1 \/ Fault="resurrect"
              IN /\ phase'=IF allowed THEN 0 ELSE phase
                 /\ current'=IF allowed THEN 1 ELSE current
                 /\ address'=IF allowed THEN 2 ELSE address
                 /\ provider'=IF allowed THEN 2 ELSE provider
                 /\ generation'=IF allowed THEN generation+1 ELSE generation
                 /\ owned'=IF allowed THEN generation+1 ELSE owned
           /\ UNCHANGED <<moved,lost,retired,dropped,oldLive,saved>>
Replace == /\ phase#2 /\ current=1 /\ current'=2 /\ address'=3 /\ provider'=0
           /\ generation'=generation+1 /\ phase'=2 /\ retired'=1 /\ oldLive'=0 /\ event'=4
           /\ UNCHANGED <<owned,moved,lost,recovered,dropped,saved>>
Retire(e) == /\ phase#2 /\ phase'=2 /\ current'=0 /\ address'=0 /\ provider'=0
             /\ retired'=1 /\ oldLive'=0 /\ event'=e
             /\ UNCHANGED <<generation,owned,moved,lost,recovered,dropped,saved>>
Drop == /\ dropped=0 /\ dropped'=1 /\ phase'=2 /\ provider'=0 /\ oldLive'=0
        /\ retired'=1 /\ event'=6 /\ saved'=current
        /\ current'=IF current=1 \/ Fault="staleDrop" THEN 0 ELSE current
        /\ address'=IF current=1 \/ Fault="staleDrop" THEN 0 ELSE address
        /\ UNCHANGED <<generation,owned,moved,lost,recovered>>
Next == Move \/ Lose \/ Recover \/ Replace \/ Drop \/ (\E e \in {5,7,8,9,10,11,12}: Retire(e))
Spec == Init /\ [][Next]_vars
TypeOK == /\ phase\in 0..2 /\ current\in 0..2 /\ address\in 0..3 /\ provider\in 0..2
          /\ generation\in 1..5 /\ owned\in 1..5 /\ moved\in 0..1 /\ lost\in 0..1
          /\ recovered\in 0..1 /\ retired\in 0..1 /\ dropped\in 0..1 /\ oldLive\in 0..1
          /\ event\in 0..12 /\ saved\in 0..2
AddressMatches == phase=0 => current=1 /\ address=provider /\ generation=owned
Suspended == phase=1 => address=0 /\ provider=0 /\ oldLive=0 /\ current=1
NoResurrection == retired=1 => phase=2 /\ provider=0
SuccessorSurvives == event=6 /\ saved=2 => current=2 /\ address=3
LiveSpec == Spec /\ WF_vars(Drop)
OwnerStops == phase#2 ~> phase=2
=============================================================================
