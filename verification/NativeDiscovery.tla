-------------------------- MODULE NativeDiscovery --------------------------
EXTENDS Naturals
CONSTANT Fault
VARIABLES generation, host, active, expired, closed, checked, foreign, stale, saved, read, event
vars == <<generation,host,active,expired,closed,checked,foreign,stale,saved,read,event>>
Init == /\ generation=0 /\ host=0 /\ active=0 /\ expired=0 /\ closed=0
        /\ checked=0 /\ foreign=0 /\ stale=0 /\ saved=0 /\ read=0 /\ event=0
Publish == /\ generation=0 /\ closed=0 /\ generation'=1 /\ host'=1 /\ active'=1 /\ event'=1
           /\ UNCHANGED <<expired,closed,checked,foreign,stale,saved,read>>
Replace(h,e) == /\ generation=1 /\ active=1 /\ closed=0 /\ generation'=2 /\ host'=h /\ expired'=0 /\ event'=e
                /\ UNCHANGED <<active,closed,checked,foreign,stale,saved,read>>
Expire == /\ active=1 /\ expired=0 /\ expired'=1 /\ event'=4
          /\ UNCHANGED <<generation,host,active,closed,checked,foreign,stale,saved,read>>
Revoke == /\ active=1 /\ active'=0 /\ event'=5
          /\ UNCHANGED <<generation,host,expired,closed,checked,foreign,stale,saved,read>>
Stale == /\ generation=2 /\ stale=0 /\ stale'=1 /\ saved'=active /\ event'=6
         /\ active'=IF Fault="staleRevoke" THEN 0 ELSE active
         /\ UNCHANGED <<generation,host,expired,closed,checked,foreign,read>>
Lookup == /\ generation>0 /\ checked=0 /\ checked'=1 /\ event'=7
          /\ read'=IF active=1 /\ closed=0 /\ (expired=0 \/ Fault="expiry") THEN host ELSE 0
          /\ UNCHANGED <<generation,host,active,expired,closed,foreign,stale,saved>>
Close == /\ closed=0 /\ closed'=1 /\ active'=0 /\ event'=8
         /\ UNCHANGED <<generation,host,expired,checked,foreign,stale,saved,read>>
Foreign == /\ generation>0 /\ foreign=0 /\ foreign'=1 /\ event'=9
           /\ read'=IF Fault="recipient" THEN host ELSE 0
           /\ UNCHANGED <<generation,host,active,expired,closed,checked,stale,saved>>
Next == Publish \/ Replace(1,2) \/ Replace(2,3) \/ Expire \/ Revoke \/ Stale \/ Lookup \/ Close \/ Foreign
Spec == Init /\ [][Next]_vars
TypeOK == /\ generation\in 0..2 /\ host\in 0..2 /\ active\in 0..1 /\ expired\in 0..1 /\ closed\in 0..1
          /\ checked\in 0..1 /\ foreign\in 0..1 /\ stale\in 0..1 /\ saved\in 0..1 /\ read\in 0..2 /\ event\in 0..9
Expiry == event=7 /\ expired=1 => read=0
Recipient == event=9 => read=0
Generation == event=6 => active=saved
Revocation == event=7 /\ (closed=1 \/ active=0) => read=0
LiveSpec == Spec /\ WF_vars(Expire)
LeaseEnds == active=1 /\ expired=0 ~> active=0 \/ expired=1
=============================================================================
