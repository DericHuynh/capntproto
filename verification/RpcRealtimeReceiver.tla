------------------------- MODULE RpcRealtimeReceiver -------------------------
EXTENDS Naturals, FiniteSets, TLC
CONSTANTS Keys, Capacity, Skew, OlderLonger, Bug
VARIABLES status, high, visible, applied, canceled, closed, time, late, afterClose, rollback,
          s1,s2,v0,v1,c1,c2,pendingCount,event,item
vars == <<status,high,visible,applied,canceled,closed,time,late,afterClose,rollback,s1,s2,v0,v1,c1,c2,pendingCount,event,item>>
Ids == 1..2
Key(n) == IF Keys=1 THEN 0 ELSE n-1
Deadline(n) == IF OlderLonger THEN (IF n=1 THEN 3 ELSE 1) ELSE 2
Timely(n) == time+Skew<Deadline(n)
Pending(s) == {n\in Ids:s[n]=1}
Record(e,n) == /\ event'=e /\ item'=n /\ s1'=status'[1] /\ s2'=status'[2]
               /\ v0'=visible'[0] /\ v1'=visible'[1] /\ c1'=applied'[1] /\ c2'=applied'[2]
               /\ pendingCount'=Cardinality(Pending(status'))
Init == /\ status=[n\in Ids|->0] /\ high=[k\in 0..1|->0] /\ visible=[k\in 0..1|->0]
        /\ applied=[n\in Ids|->0] /\ canceled={} /\ closed=FALSE /\ time=0
        /\ late=FALSE /\ afterClose=FALSE /\ rollback=FALSE
        /\ s1=0 /\ s2=0 /\ v0=0 /\ v1=0 /\ c1=0 /\ c2=0 /\ pendingCount=0 /\ event=0 /\ item=0
Offer(n) ==
    LET remember==status[n]#0 /\ ~(Bug="forgetCancel" /\ status[n]=6) /\ ~(Bug="duplicate" /\ status[n]=2)
        stale==n<high[Key(n)] /\ Bug#"stale"
        advance==~remember /\ ~closed /\ ~stale
        replaced==[m\in Ids|->IF m#n /\ Key(m)=Key(n) /\ status[m]=1 THEN 4 ELSE status[m]]
        outcome==IF ~Timely(n) THEN 3 ELSE IF Cardinality(Pending(replaced))>=Capacity /\ Bug#"overflow" THEN 5 ELSE 1
    IN /\ status'=(IF remember THEN status ELSE IF closed THEN [status EXCEPT ![n]=7]
                  ELSE IF stale THEN [status EXCEPT ![n]=4] ELSE [replaced EXCEPT ![n]=outcome])
       /\ high'=(IF advance THEN [high EXCEPT ![Key(n)]=n] ELSE high)
       /\ UNCHANGED <<visible,applied,canceled,closed,time,late,afterClose,rollback>> /\ Record(1,n)
Apply(n) == /\ status[n]=1 /\ (~closed \/ Bug="closeLeak")
    /\ LET outcome==IF Timely(n) \/ Bug="late" THEN 2 ELSE 3
       IN /\ status'=[status EXCEPT ![n]=outcome]
          /\ visible'=(IF outcome=2 THEN [visible EXCEPT ![Key(n)]=n] ELSE visible)
          /\ applied'=(IF outcome=2 THEN [applied EXCEPT ![n]=@+1] ELSE applied)
          /\ late'=(late \/ (outcome=2 /\ ~Timely(n)))
          /\ afterClose'=(afterClose \/ (outcome=2 /\ closed))
          /\ rollback'=(rollback \/ (outcome=2 /\ n<visible[Key(n)]))
    /\ UNCHANGED <<high,canceled,closed,time>> /\ Record(2,n)
Expire == /\ \E n\in Ids:status[n]=1 /\ ~Timely(n)
          /\ status'=[n\in Ids|->IF status[n]=1 /\ ~Timely(n) THEN 3 ELSE status[n]]
          /\ UNCHANGED <<high,visible,applied,canceled,closed,time,late,afterClose,rollback>> /\ Record(3,0)
Cancel(n) == /\ status'=(IF status[n]\in {0,1} THEN [status EXCEPT ![n]=IF closed THEN 7 ELSE 6] ELSE status)
             /\ canceled'=(IF status[n]\in {0,1} /\ ~closed THEN canceled\cup{n} ELSE canceled)
             /\ UNCHANGED <<high,visible,applied,closed,time,late,afterClose,rollback>> /\ Record(4,n)
Close == /\ ~closed /\ closed'=TRUE
         /\ status'=[n\in Ids|->IF status[n]=1 /\ Bug#"closeLeak" THEN 7 ELSE status[n]]
         /\ UNCHANGED <<high,visible,applied,canceled,time,late,afterClose,rollback>> /\ Record(5,0)
Tick == /\ time<3 /\ time'=time+1 /\ UNCHANGED <<status,high,visible,applied,canceled,closed,late,afterClose,rollback>> /\ Record(6,0)
Next == (\E n\in Ids:Offer(n) \/ Apply(n) \/ Cancel(n)) \/ Expire \/ Close \/ Tick
Spec == Init /\ [][Next]_vars
TypeOK == /\ status\in [Ids->0..7] /\ high\in [0..1->0..2] /\ visible\in [0..1->0..2]
          /\ applied\in [Ids->0..2] /\ canceled\subseteq Ids /\ closed\in BOOLEAN /\ time\in 0..3
          /\ late\in BOOLEAN /\ afterClose\in BOOLEAN /\ rollback\in BOOLEAN
          /\ s1=status[1] /\ s2=status[2] /\ v0=visible[0] /\ v1=visible[1] /\ c1=applied[1] /\ c2=applied[2]
          /\ pendingCount=Cardinality(Pending(status)) /\ event\in 0..6 /\ item\in 0..2
NoLate == ~late
NoAfterClose == ~afterClose
NoRollback == ~rollback
AtMostOnce == \A n\in Ids:applied[n]<=1
QueueBound == Cardinality(Pending(status))<=Capacity
PendingFresh == \A n\in Pending(status):n=high[Key(n)]
NoCanceled == \A n\in canceled:applied[n]=0
OutcomeMatches == \A n\in Ids:(status[n]=2) <=> (applied[n]=1)
=============================================================================
