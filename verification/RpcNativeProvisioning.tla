----------------------- MODULE RpcNativeProvisioning -----------------------
EXTENDS Naturals, FiniteSets, TLC
CONSTANT Bug
VARIABLES p,h,w,v,ack,closed,route,badCancel,badRevoke,event,item,
          p1,p2,h1,h2,w1,w2,v1,v2,ack1,ack2
vars == <<p,h,w,v,ack,closed,route,badCancel,badRevoke,event,item,
          p1,p2,h1,h2,w1,w2,v1,v2,ack1,ack2>>
Ids == 1..2
Active == {i \in Ids: p[i] \in {1,2}}
\* One delegated provider, two successive leases, one ready waiter per lease.
\* Phases: unissued, pending, installing, installed, retired. Authentication
\* proof and the network attach result abstract the existing listener/network.
Init == /\ p=[i\in Ids|->0] /\ h=[i\in Ids|->FALSE] /\ w=p /\ v=h /\ ack=p
        /\ closed=FALSE /\ route=0 /\ badCancel=FALSE /\ badRevoke=FALSE /\ event=0 /\ item=0
        /\ p1=0 /\ p2=0 /\ h1=FALSE /\ h2=FALSE /\ w1=0 /\ w2=0
        /\ v1=FALSE /\ v2=FALSE /\ ack1=0 /\ ack2=0
Reserve(i) == /\ p[i]=0 /\ event'=1 /\ item'=i
              /\ LET ok == (~closed \/ Bug="closedIssue") /\ route=0 /\ (Active={} \/ Bug="overlap")
                 IN /\ p'=IF ok THEN [p EXCEPT ![i]=1] ELSE p
                    /\ h'=IF ok THEN [h EXCEPT ![i]=TRUE] ELSE h
              /\ UNCHANGED <<w,v,ack,closed,route,badCancel,badRevoke>>
Wait(i) == /\ h[i] /\ w[i]=0 /\ ack[i]=0 /\ event'=2 /\ item'=i
           /\ w'=[w EXCEPT ![i]=1] /\ UNCHANGED <<p,h,v,ack,closed,route,badCancel,badRevoke>>
Authenticate(i) == /\ p[i]=1 /\ event'=3 /\ item'=i
                   /\ p'=[p EXCEPT ![i]=2] /\ v'=[v EXCEPT ![i]=TRUE]
                   /\ UNCHANGED <<h,w,ack,closed,route,badCancel,badRevoke>>
Install(i) == /\ (p[i]=2 \/ (Bug="unauthed" /\ p[i]=1)) /\ event'=4 /\ item'=i
              /\ p'=[p EXCEPT ![i]=IF route=0 THEN 3 ELSE 4]
              /\ route'=IF route=0 THEN i ELSE route
              /\ UNCHANGED <<h,w,v,ack,closed,badCancel,badRevoke>>
FailInstall(i) == /\ p[i]=2 /\ event'=5 /\ item'=i
                 /\ p'=[p EXCEPT ![i]=4] /\ UNCHANGED <<h,w,v,ack,closed,route,badCancel,badRevoke>>
Cancel(i) == /\ h[i] /\ event'=6 /\ item'=i
             /\ LET j == IF Bug="staleCancel" /\ i=1 /\ p[1]=4 /\ p[2]=1 THEN 2 ELSE i
                IN /\ p'=IF p[j]=1 THEN [p EXCEPT ![j]=4] ELSE p
                   /\ badCancel'=(badCancel \/ j#i)
             /\ UNCHANGED <<h,w,v,ack,closed,route,badRevoke>>
Drop(i) == /\ h[i] /\ event'=7 /\ item'=i /\ h'=[h EXCEPT ![i]=FALSE]
           /\ p'=IF p[i]=1 /\ w[i]=0 /\ Bug#"dropLeak" THEN [p EXCEPT ![i]=4] ELSE p
           /\ UNCHANGED <<w,v,ack,closed,route,badCancel,badRevoke>>
CancelWait(i) == /\ w[i]=1 /\ event'=8 /\ item'=i /\ w'=[w EXCEPT ![i]=0]
                 /\ p'=IF p[i]=1 /\ ~h[i] THEN [p EXCEPT ![i]=4] ELSE p
                 /\ UNCHANGED <<h,v,ack,closed,route,badCancel,badRevoke>>
Ready(i) == /\ w[i]=1 /\ (p[i]\in {3,4} \/ Bug="earlyAck") /\ event'=9 /\ item'=i
            /\ w'=[w EXCEPT ![i]=0] /\ ack'=[ack EXCEPT ![i]=IF p[i]=4 THEN 3 ELSE 2]
            /\ UNCHANGED <<p,h,v,closed,route,badCancel,badRevoke>>
Expire(i) == /\ p[i]=1 /\ event'=10 /\ item'=i /\ p'=[p EXCEPT ![i]=4]
             /\ UNCHANGED <<h,w,v,ack,closed,route,badCancel,badRevoke>>
Close == /\ ~closed /\ closed'=TRUE /\ event'=11 /\ item'=0
         /\ p'=[i\in Ids|->IF p[i]=1 THEN 4 ELSE p[i]]
         /\ route'=IF Bug="closeRoute" THEN 0 ELSE route
         /\ badRevoke'=(badRevoke \/ (Bug="closeRoute" /\ route#0))
         /\ UNCHANGED <<h,w,v,ack,badCancel>>
Disconnect == /\ route#0 /\ route'=0 /\ event'=12 /\ item'=0
              /\ UNCHANGED <<p,h,w,v,ack,closed,badCancel,badRevoke>>
LateAuthenticate(i) == /\ p[i]=4 /\ h[i] /\ event'=13 /\ item'=i
                      /\ UNCHANGED <<p,h,w,v,ack,closed,route,badCancel,badRevoke>>
Project == /\ p1'=p'[1] /\ p2'=p'[2] /\ h1'=h'[1] /\ h2'=h'[2]
           /\ w1'=w'[1] /\ w2'=w'[2] /\ v1'=v'[1] /\ v2'=v'[2]
           /\ ack1'=ack'[1] /\ ack2'=ack'[2]
Next == ((\E i\in Ids: Reserve(i) \/ Wait(i) \/ Authenticate(i) \/ Install(i) \/ FailInstall(i)
              \/ Cancel(i) \/ Drop(i) \/ CancelWait(i) \/ Ready(i) \/ Expire(i) \/ LateAuthenticate(i))
         \/ Close \/ Disconnect) /\ Project
Spec == Init /\ [][Next]_vars
\* Progress assumes the local executor services expiry, synchronous installation
\* and notified ready waiters. It does not assume the peer completes Native.
FairSpec == Spec /\ (\A i\in Ids:
              /\ IF Bug="noExpiry" THEN TRUE ELSE WF_vars(Expire(i) /\ Project)
              /\ WF_vars((Install(i) \/ FailInstall(i)) /\ Project)
              /\ WF_vars(Ready(i) /\ Project))
Settled == \A i\in Ids:(p[i]\in {1,2}) ~> (p[i]\in {3,4})
WaitersFinish == \A i\in Ids:(w[i]=1) ~> (w[i]=0)
TypeOK == /\ p\in [Ids->0..4] /\ h\in [Ids->BOOLEAN] /\ w\in [Ids->0..1]
          /\ v\in [Ids->BOOLEAN] /\ ack\in [Ids->{0,2,3}] /\ closed\in BOOLEAN
          /\ route\in 0..2 /\ badCancel\in BOOLEAN /\ badRevoke\in BOOLEAN
          /\ event\in 0..13 /\ item\in 0..2
Quota == Cardinality(Active)<=1
Revoked == closed => \A i\in Ids:p[i]#1
Authenticated == /\ \A i\in Ids:p[i]\in {2,3} => v[i]
                 /\ route#0 => p[route]=3 /\ v[route]
Acknowledged == \A i\in Ids:ack[i]=2 => p[i]=3 /\ v[i]
Isolated == ~badCancel
InstalledLifetime == ~badRevoke
Owned == \A i\in Ids:p[i]=1 => h[i] \/ w[i]=1
=============================================================================
