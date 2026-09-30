------------------------- MODULE RpcNoiseListener -------------------------
EXTENDS Naturals, FiniteSets, TLC
CONSTANTS Slots, Queue, Bug
VARIABLES p,q,d,v,time,closed,stopped,badRoute,badRelease,event,item,
          p1,p2,q1,q2,d1,d2,v1,v2
vars == <<p,q,d,v,time,closed,stopped,badRoute,badRelease,event,item,p1,p2,q1,q2,d1,d2,v1,v2>>
Ids == 1..2
Active == {i\in Ids:p[i]\in {1,2}}
\* Phases: unissued, reserved, authenticated, retired. Initial IDs are never
\* reused. Handshake cryptography is abstract: only Authenticate has a proof.
Init == /\ p=[i\in Ids|->0] /\ q=p /\ d=p /\ v=[i\in Ids|->FALSE]
        /\ time=0 /\ closed=FALSE /\ stopped=FALSE /\ badRoute=FALSE /\ badRelease=FALSE /\ event=0 /\ item=0
        /\ p1=0 /\ p2=0 /\ q1=0 /\ q2=0 /\ d1=0 /\ d2=0 /\ v1=FALSE /\ v2=FALSE
Reserve(i) == /\ ~closed /\ p[i]=0 /\ event'=1 /\ item'=i
              /\ p'=IF Cardinality(Active)<Slots /\ (~stopped \/ Bug="lateReserve") THEN [p EXCEPT ![i]=1] ELSE p
              /\ d'=IF Cardinality(Active)<Slots /\ (~stopped \/ Bug="lateReserve") THEN [d EXCEPT ![i]=time+1] ELSE d
              /\ UNCHANGED <<q,v,time,closed,stopped,badRoute,badRelease>>
Enqueue(i) == /\ event'=2 /\ item'=i
              /\ LET j == IF Bug="misroute" THEN 3-i ELSE i
                     send == ~closed /\ i\in Active /\ j\in Active /\ (q[j]<Queue \/ Bug="overflow")
                 IN /\ q'=IF send THEN [q EXCEPT ![j]=@+1] ELSE q
                    /\ badRoute'=(badRoute \/ (send /\ i#j))
              /\ UNCHANGED <<p,d,v,time,closed,stopped,badRelease>>
Read(i) == /\ i\in Active /\ q[i]>0 /\ event'=3 /\ item'=i
           /\ q'=[q EXCEPT ![i]=@-1] /\ UNCHANGED <<p,d,v,time,closed,stopped,badRoute,badRelease>>
Authenticate(i) == /\ p[i]=1 /\ event'=4 /\ item'=i
                   /\ p'=[p EXCEPT ![i]=2] /\ d'=[d EXCEPT ![i]=0] /\ v'=[v EXCEPT ![i]=TRUE]
                   /\ UNCHANGED <<q,time,closed,stopped,badRoute,badRelease>>
Reject(i) == /\ p[i]=1 /\ event'=5 /\ item'=i
             /\ p'=[p EXCEPT ![i]=IF Bug="wrongPeer" THEN 2 ELSE 3]
             /\ d'=[d EXCEPT ![i]=0] /\ q'=[q EXCEPT ![i]=0]
             /\ UNCHANGED <<v,time,closed,stopped,badRoute,badRelease>>
Release(i) == /\ p[i]#0 /\ event'=6 /\ item'=i
              /\ LET j == IF Bug="staleDrop" /\ i=1 /\ p[1]=3 /\ 2\in Active THEN 2 ELSE i
                 IN /\ p'=[p EXCEPT ![j]=3] /\ d'=[d EXCEPT ![j]=0] /\ q'=[q EXCEPT ![j]=0]
                    /\ badRelease'=(badRelease \/ i#j)
              /\ UNCHANGED <<v,time,closed,stopped,badRoute>>
Tick == /\ time<2 /\ time'=time+1 /\ event'=7 /\ item'=0
        /\ LET expired == {i\in Ids:p[i]=1 /\ d[i]<=time' /\ Bug#"expiryLeak"}
           IN /\ p'=[i\in Ids|->IF i\in expired THEN 3 ELSE p[i]]
              /\ q'=[i\in Ids|->IF i\in expired THEN 0 ELSE q[i]]
              /\ d'=[i\in Ids|->IF i\in expired THEN 0 ELSE d[i]]
        /\ UNCHANGED <<v,closed,stopped,badRoute,badRelease>>
Unknown == /\ event'=8 /\ item'=0 /\ UNCHANGED <<p,q,d,v,time,closed,stopped,badRoute,badRelease>>
LateAuthenticate(i) == /\ p[i]=3 /\ event'=9 /\ item'=i
                      /\ UNCHANGED <<p,q,d,v,time,closed,stopped,badRoute,badRelease>>
Close == /\ ~closed /\ closed'=TRUE /\ stopped'=TRUE /\ event'=10 /\ item'=0
         /\ p'=[i\in Ids|->IF i\in Active /\ Bug#"closeLeak" THEN 3 ELSE p[i]]
         /\ q'=IF Bug="closeLeak" THEN q ELSE [i\in Ids|->0]
         /\ d'=IF Bug="closeLeak" THEN d ELSE [i\in Ids|->0]
         /\ UNCHANGED <<v,time,badRoute,badRelease>>
Stop == /\ ~stopped /\ stopped'=TRUE /\ event'=11 /\ item'=0
        /\ LET retire == {i\in Ids:(p[i]=1 /\ Bug#"stopLeak") \/ (p[i]=2 /\ Bug="stopAuthenticated")}
           IN /\ p'=[i\in Ids|->IF i\in retire THEN 3 ELSE p[i]]
              /\ q'=[i\in Ids|->IF i\in retire THEN 0 ELSE q[i]]
              /\ d'=[i\in Ids|->IF i\in retire THEN 0 ELSE d[i]]
        /\ badRelease'=(badRelease \/ (Bug="stopAuthenticated" /\ \E i\in Ids:p[i]=2))
        /\ UNCHANGED <<v,time,closed,badRoute>>
Project == /\ p1'=p'[1] /\ p2'=p'[2] /\ q1'=q'[1] /\ q2'=q'[2]
           /\ d1'=d'[1] /\ d2'=d'[2] /\ v1'=v'[1] /\ v2'=v'[2]
Next == ((\E i\in Ids: Reserve(i) \/ Enqueue(i) \/ Read(i) \/ Authenticate(i) \/ Reject(i) \/ Release(i) \/ LateAuthenticate(i))
         \/ Tick \/ Unknown \/ Close \/ Stop) /\ Project
Spec == Init /\ [][Next]_vars
TypeOK == /\ p\in [Ids->0..3] /\ q\in [Ids->0..(Queue+1)] /\ d\in [Ids->0..3] /\ v\in [Ids->BOOLEAN]
          /\ time\in 0..2 /\ closed\in BOOLEAN /\ badRoute\in BOOLEAN /\ badRelease\in BOOLEAN /\ stopped\in BOOLEAN /\ event\in 0..11 /\ item\in 0..2
Stopped == stopped => \A i\in Ids:p[i]#1
Quota == /\ Cardinality(Active)<=Slots /\ \A i\in Ids:q[i]<=Queue
Isolated == ~badRoute /\ ~badRelease
Authenticated == \A i\in Ids:p[i]=2 => v[i]
Expired == \A i\in Ids:p[i]=1 => time<d[i]
Lifetime == /\ closed => Active={}
            /\ \A i\in Ids:p[i]\in {0,3} => q[i]=0 /\ d[i]=0
=============================================================================
