------------------------- MODULE RealtimeReceiptLifecycle -------------------------
EXTENDS Naturals
CONSTANT Fault
\* One key, two sequences/data values, two retained receipts and transient probes.
\* Status: absent=0, queued=1, applied=2, expired=3, superseded=4,
\* canceled=6, closed=7. Four operations; time 0..2, both deadlines at 2.
VARIABLES s1,s2,m1,m2,high,visible,c1,c2,now,closed,a,b,w,steps,event,item,slot,data,
          result,expected,prior1,prior2
vars == <<s1,s2,m1,m2,high,visible,c1,c2,now,closed,a,b,w,steps,event,item,slot,data,
          result,expected,prior1,prior2>>
Status(s) == IF s=1 THEN s1 ELSE s2
Metadata(s) == IF s=1 THEN m1 ELSE m2
Count(x,y,l,r) == (IF (l=1 /\ x=1) \/ (l=2 /\ y=1) THEN 1 ELSE 0)
                 + (IF (r=1 /\ x=1) \/ (r=2 /\ y=1) THEN 1 ELSE 0)
Init == /\ s1=0 /\ s2=0 /\ m1=0 /\ m2=0 /\ high=0 /\ visible=0 /\ c1=0 /\ c2=0
        /\ now=0 /\ closed=0 /\ a=0 /\ b=0 /\ w=0 /\ steps=0 /\ event=0
        /\ item=0 /\ slot=0 /\ data=0 /\ result=0 /\ expected=0 /\ prior1=0 /\ prior2=0
Advance(e) == /\ steps<4 /\ steps'=steps+1 /\ event'=e /\ prior1'=s1 /\ prior2'=s2
              /\ w'=(IF Fault="leak" /\ ((s1=1 /\ s1'>1) \/ (s2=1 /\ s2'>1))
                      THEN w ELSE Count(s1',s2',a',b'))
Answer(s,k,broken) ==
    IF Metadata(s)#0 /\ Metadata(s)#k /\ ~(broken /\ Fault="conflict") THEN 0
    ELSE IF Status(s)>=2 /\ ~(broken /\ Fault="revive" /\ Metadata(s)=0) THEN Status(s)
    ELSE IF w>=2 /\ ~(broken /\ Fault="quota") THEN 0
    ELSE IF Status(s)=1 THEN 1
    ELSE IF closed=1 THEN 7
    ELSE IF s<high THEN 4
    ELSE IF now=2 THEN 3
    ELSE 1
Offer(s,k,t) ==
    /\ (IF t=1 THEN a=0 ELSE IF t=2 THEN b=0 ELSE TRUE)
    /\ item'=s /\ slot'=t /\ data'=k
    /\ result'=Answer(s,k,TRUE) /\ expected'=Answer(s,k,FALSE)
    /\ LET fresh == Status(s)=0 \/ (Fault="revive" /\ Metadata(s)=0)
           admit == fresh /\ result'>0
           replace == admit /\ result'\in {1,3}
       IN /\ s1'=(IF admit /\ s=1 THEN result' ELSE IF replace /\ s1=1 THEN 4 ELSE s1)
          /\ s2'=(IF admit /\ s=2 THEN result' ELSE IF replace /\ s2=1 THEN 4 ELSE s2)
          /\ m1'=(IF admit /\ s=1 THEN k ELSE m1)
          /\ m2'=(IF admit /\ s=2 THEN k ELSE m2)
          /\ high'=(IF replace THEN s ELSE high)
    /\ a'=(IF t=1 /\ result'>0 THEN s ELSE a)
    /\ b'=(IF t=2 /\ result'>0 THEN s ELSE b)
    /\ UNCHANGED <<visible,c1,c2,now,closed>> /\ Advance(1)
Apply(s) ==
    /\ item'=s /\ slot'=0 /\ data'=0
    /\ result'=(IF Status(s)=1 THEN IF now=2 THEN 3 ELSE 2 ELSE Status(s))
    /\ expected'=result'
    /\ s1'=(IF s=1 THEN result' ELSE s1) /\ s2'=(IF s=2 THEN result' ELSE s2)
    /\ LET apply == result'=2 /\ (Status(s)=1 \/ Fault="duplicate")
       IN /\ visible'=(IF apply THEN s ELSE visible)
          /\ c1'=(IF apply /\ s=1 THEN c1+1 ELSE c1)
          /\ c2'=(IF apply /\ s=2 THEN c2+1 ELSE c2)
    /\ UNCHANGED <<m1,m2,high,now,closed,a,b>> /\ Advance(2)
Cancel(s) ==
    /\ item'=s /\ slot'=0 /\ data'=0
    /\ result'=(IF Status(s)>=2 /\ Fault#"overwrite" THEN Status(s) ELSE IF closed=1 THEN 7 ELSE 6)
    /\ expected'=result'
    /\ s1'=(IF s=1 THEN result' ELSE s1) /\ s2'=(IF s=2 THEN result' ELSE s2)
    /\ UNCHANGED <<m1,m2,high,visible,c1,c2,now,closed,a,b>> /\ Advance(3)
Close ==
    /\ item'=0 /\ slot'=0 /\ data'=0 /\ result'=8 /\ expected'=8 /\ closed'=1
    /\ s1'=(IF s1=1 THEN 7 ELSE s1) /\ s2'=(IF s2=1 THEN 7 ELSE s2)
    /\ UNCHANGED <<m1,m2,high,visible,c1,c2,now,a,b>> /\ Advance(4)
Drop(t) ==
    /\ (IF t=1 THEN a#0 ELSE b#0)
    /\ item'=0 /\ slot'=t /\ data'=0 /\ result'=8 /\ expected'=8
    /\ a'=(IF t=1 THEN 0 ELSE a) /\ b'=(IF t=2 THEN 0 ELSE b)
    /\ LET s==IF t=1 THEN a ELSE b
       IN /\ s1'=(IF Fault="drop" /\ s=1 /\ s1=1 THEN 6 ELSE s1)
          /\ s2'=(IF Fault="drop" /\ s=2 /\ s2=1 THEN 6 ELSE s2)
    /\ UNCHANGED <<m1,m2,high,visible,c1,c2,now,closed>> /\ Advance(5)
Tick ==
    /\ now<2 /\ now'=now+1 /\ item'=0 /\ slot'=0 /\ data'=0 /\ result'=8 /\ expected'=8
    /\ UNCHANGED <<s1,s2,m1,m2,high,visible,c1,c2,closed,a,b>> /\ Advance(6)
Expire ==
    /\ item'=0 /\ slot'=0 /\ data'=0 /\ result'=8 /\ expected'=8
    /\ s1'=(IF now=2 /\ s1=1 THEN 3 ELSE s1) /\ s2'=(IF now=2 /\ s2=1 THEN 3 ELSE s2)
    /\ UNCHANGED <<m1,m2,high,visible,c1,c2,now,closed,a,b>> /\ Advance(7)
Next == (\E s,k\in 1..2,t\in 1..3: Offer(s,k,t)) \/ (\E s\in 1..2: Apply(s) \/ Cancel(s))
        \/ Close \/ (\E t\in 1..2: Drop(t)) \/ Tick \/ Expire
Spec == Init /\ [][Next]_vars
TypeOK == /\ s1\in {0,1,2,3,4,6,7} /\ s2\in {0,1,2,3,4,6,7}
          /\ m1\in 0..2 /\ m2\in 0..2 /\ high\in 0..2 /\ visible\in 0..2
          /\ c1\in 0..4 /\ c2\in 0..4 /\ now\in 0..2 /\ closed\in 0..1
          /\ a\in 0..2 /\ b\in 0..2 /\ w\in 0..2 /\ steps\in 0..4 /\ event\in 0..7
          /\ item\in 0..2 /\ slot\in 0..3 /\ data\in 0..2 /\ result\in 0..8 /\ expected\in 0..8
          /\ prior1\in {0,1,2,3,4,6,7} /\ prior2\in {0,1,2,3,4,6,7}
ExpectedOffer == event=1 => result=expected
WaiterConservation == w=Count(s1,s2,a,b)
DropPreservesWork == event=5 => s1=prior1 /\ s2=prior2
TerminalStable == (prior1>=2 => s1=prior1) /\ (prior2>=2 => s2=prior2)
AppliedOnce == c1<=1 /\ c2<=1
QueuedPayload == (s1=1 => m1#0) /\ (s2=1 => m2#0)
ClosedDrains == closed=1 => s1#1 /\ s2#1
=============================================================================
