------------------------- MODULE RpcOutgoingQueue -------------------------
EXTENDS Naturals
CONSTANT Fault
\* Two ordered messages of one/two words; batch dispatch, blocked writes,
\* flush, receipt cancellation, shutdown, I/O failure and driver cancellation.
\* m: 0 absent, 1 queued, 2 writing, 3 flushing, 4 complete, 5 failed.
\* r: 0 absent, 1 pending, 2 successful, 3 failed, 4 canceled.
VARIABLES m1,m2,r1,r2,count,bytes,now,oldest,t1,t2,closing,done,rejected,event
vars == <<m1,m2,r1,r2,count,bytes,now,oldest,t1,t2,closing,done,rejected,event>>
Count(x,y) == (IF x=1 THEN 1 ELSE 0)+(IF y=1 THEN 1 ELSE 0)
Bytes(x,y) == (IF x=1 THEN 8 ELSE 0)+(IF y=1 THEN 16 ELSE 0)
Active == m1\in {2,3} \/ m2\in {2,3}
Pending == m1\in 1..3 \/ m2\in 1..3
Init == /\ m1=0 /\ m2=0 /\ r1=0 /\ r2=0 /\ count=0 /\ bytes=0
        /\ now=0 /\ oldest=0 /\ t1=0 /\ t2=0
        /\ closing=0 /\ done=0 /\ rejected=0 /\ event=0
Send == /\ closing=0 /\ m2=0
        /\ m1'=IF m1=0 THEN 1 ELSE m1
        /\ m2'=IF m1=0 THEN 0 ELSE 1
        /\ r1'=IF m1=0 THEN 1 ELSE r1
        /\ r2'=IF m1=0 THEN 0 ELSE 1
        /\ t1'=IF m1=0 THEN now ELSE t1
        /\ t2'=IF m1=0 THEN t2 ELSE now
        /\ count'=count+1 /\ bytes'=bytes+(IF m1=0 THEN 8 ELSE 16)
        /\ oldest'=IF count=0 /\ Fault#"staleAge" THEN now ELSE oldest
        /\ event'=1 /\ UNCHANGED <<now,closing,done,rejected>>
Tick == /\ now<1 /\ now'=now+1 /\ event'=2
        /\ UNCHANGED <<m1,m2,r1,r2,count,bytes,oldest,t1,t2,closing,done,rejected>>
Start == /\ ~Active /\ count>0 /\ closing#2
         /\ m1'=IF m1=1 THEN 2 ELSE m1
         /\ m2'=IF m2=1 THEN 2 ELSE m2
         /\ count'=IF Fault="activeCounts" THEN count ELSE 0
         /\ bytes'=IF Fault="activeCounts" THEN bytes ELSE 0
         /\ oldest'=0 /\ event'=3
         /\ UNCHANGED <<r1,r2,now,t1,t2,closing,done,rejected>>
Write == /\ m1=2 \/ m2=2
         /\ m1'=IF m1=2 THEN 3 ELSE m1
         /\ m2'=IF m2=2 THEN 3 ELSE m2
         /\ r1'=IF Fault="earlyReceipt" /\ m1=2 THEN 2 ELSE r1
         /\ r2'=IF Fault="earlyReceipt" /\ m2=2 /\ r2#4 THEN 2 ELSE r2
         /\ event'=4
         /\ UNCHANGED <<count,bytes,now,oldest,t1,t2,closing,done,rejected>>
Flush == /\ m1=3 \/ m2=3
         /\ m1'=IF m1=3 THEN 4 ELSE IF m1=1 THEN 2 ELSE m1
         /\ m2'=IF m2=3 THEN 4 ELSE IF m2=1 THEN 2 ELSE m2
         /\ r1'=IF m1=3 THEN 2 ELSE r1
         /\ r2'=IF m2=3 /\ r2#4 THEN 2 ELSE r2
         /\ done'=IF closing=1 /\ count=0 THEN 1 ELSE done
         /\ closing'=IF closing=1 /\ count=0 THEN 2 ELSE closing
         /\ count'=0 /\ bytes'=0 /\ oldest'=0 /\ event'=5
         /\ UNCHANGED <<now,t1,t2,rejected>>
Terminate == /\ closing=0 /\ closing'=1 /\ event'=6
             /\ UNCHANGED <<m1,m2,r1,r2,count,bytes,now,oldest,t1,t2,done,rejected>>
Reject == /\ closing#0 /\ rejected=0
          /\ rejected'=IF Fault="lateSend" THEN 2 ELSE 1
          /\ event'=7
          /\ UNCHANGED <<m1,m2,r1,r2,count,bytes,now,oldest,t1,t2,closing,done>>
Cancel == /\ r2=1 /\ r2'=4 /\ event'=8
          /\ m2'=IF Fault="cancelMessage" THEN 5 ELSE m2
          /\ UNCHANGED <<m1,r1,count,bytes,now,oldest,t1,t2,closing,done,rejected>>
Fail(e) == /\ closing#2
           /\ (IF e=9 THEN m1=2 \/ m2=2 ELSE IF e=10 THEN m1=3 \/ m2=3 ELSE TRUE)
           /\ m1'=IF m1\in 1..3 THEN 5 ELSE m1
           /\ m2'=IF m2\in 1..3 THEN 5 ELSE m2
           /\ r1'=IF r1=1 THEN 3 ELSE r1
           /\ r2'=IF r2=1 THEN 3 ELSE r2
           /\ count'=IF Fault="staleFailure" THEN count ELSE 0
           /\ bytes'=IF Fault="staleFailure" THEN bytes ELSE 0
           /\ oldest'=0 /\ closing'=2 /\ done'=2 /\ event'=e
           /\ UNCHANGED <<now,t1,t2,rejected>>
Finish == /\ closing=1 /\ ~Pending /\ closing'=2 /\ done'=1 /\ event'=12
          /\ UNCHANGED <<m1,m2,r1,r2,count,bytes,now,oldest,t1,t2,rejected>>
Next == Send \/ Tick \/ Start \/ Write \/ Flush \/ Terminate \/ Reject \/ Cancel
        \/ Fail(9) \/ Fail(10) \/ Fail(11) \/ Finish
Spec == Init /\ [][Next]_vars
Advance == Start \/ Write \/ Flush \/ Finish
LiveSpec == Spec /\ WF_vars(Advance)
DrainProgress == closing=1 ~> closing=2
TypeOK == /\ m1\in 0..5 /\ m2\in 0..5 /\ r1\in 0..4 /\ r2\in 0..4
          /\ count\in 0..2 /\ bytes\in {0,8,16,24} /\ now\in 0..1
          /\ oldest\in 0..1 /\ t1\in 0..1 /\ t2\in 0..1
          /\ closing\in 0..2 /\ done\in 0..2 /\ rejected\in 0..2 /\ event\in 0..12
Metrics == count=Count(m1,m2) /\ bytes=Bytes(m1,m2)
QueueAge == count>0 => oldest=(IF m1=1 THEN t1 ELSE t2)
FlushFence == (r1=2 => m1=4) /\ (r2=2 => m2=4)
Admission == rejected#2
CancellationOwnership == event=8 => m2\in 1..3
ClosedIsSettled == closing=2 => ~Pending /\ r1#1 /\ r2#1
Fifo == m2=4 => m1=4
=============================================================================
