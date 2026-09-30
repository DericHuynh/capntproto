--------------------------- MODULE RpcBulkTransfer ---------------------------
EXTENDS Integers, Sequences, FiniteSets, TLC
CONSTANTS Window, FailAt, EarlyDone, ReorderReplies, Bug
ASSUME /\ Window\in {2,3} /\ FailAt\in 0..2 /\ EarlyDone\in BOOLEAN
       /\ ReorderReplies\in BOOLEAN
       /\ Bug\in {"none","ignoreWindow","doubleCredit","forgetError","earlyPublish","incomplete","cancelLeak"}
Ids == 1..2
Size(n) == n
Bytes(ids) == (IF 1\in ids THEN 1 ELSE 0)+(IF 2\in ids THEN 2 ELSE 0)
Msg(k,n,r) == [kind|->k,seq|->n,result|->r]
VARIABLES phase,sent,acked,credit,requests,replies,failed,server,staged,visible,
          acceptedBytes,acceptedCount,results,doneSent,cancelSent,duplicated,clientError,
          final,cancelResult,event,item,sentCount,ackCount,r1,r2
vars == <<phase,sent,acked,credit,requests,replies,failed,server,staged,visible,
          acceptedBytes,acceptedCount,results,doneSent,cancelSent,duplicated,clientError,
          final,cancelResult,event,item,sentCount,ackCount,r1,r2>>
Record(e,n) == /\ event'=e /\ item'=n
Project == /\ sentCount'=Cardinality(sent') /\ ackCount'=Cardinality(acked')
           /\ r1'=results'[1] /\ r2'=results'[2]
Init == /\ phase=0 /\ sent={} /\ acked={} /\ credit=Window /\ requests= <<>> /\ replies= <<>>
        /\ failed=FALSE /\ server=0 /\ staged=0 /\ visible=0 /\ acceptedBytes=0 /\ acceptedCount=0
        /\ results=[n\in Ids|->0] /\ doneSent=FALSE /\ cancelSent=FALSE /\ duplicated=FALSE
        /\ clientError=FALSE /\ final=0 /\ cancelResult=0 /\ event=0 /\ item=0
        /\ sentCount=0 /\ ackCount=0 /\ r1=0 /\ r2=0
Send == /\ phase=0 /\ ~clientError /\ sent#Ids
        /\ LET n==Cardinality(sent)+1 IN
           /\ (credit>=Size(n) \/ Bug="ignoreWindow") /\ credit'=credit-Size(n)
           /\ sent'=sent\cup{n} /\ requests'=Append(requests,Msg(1,n,0)) /\ Record(1,n)
        /\ UNCHANGED <<phase,acked,replies,failed,server,staged,visible,acceptedBytes,acceptedCount,
                       results,doneSent,cancelSent,duplicated,clientError,final,cancelResult>>
Done == /\ phase=0 /\ (sent=Ids \/ EarlyDone) /\ ~doneSent
        /\ phase'=1 /\ doneSent'=TRUE /\ requests'=Append(requests,Msg(2,0,0))
        /\ UNCHANGED <<sent,acked,credit,replies,failed,server,staged,visible,acceptedBytes,acceptedCount,
                       results,cancelSent,duplicated,clientError,final,cancelResult>> /\ Record(2,0)
Cancel == /\ phase\in {0,1,3} /\ ~cancelSent
          /\ phase'=2 /\ cancelSent'=TRUE /\ requests'=Append(requests,Msg(3,0,0))
          /\ UNCHANGED <<sent,acked,credit,replies,failed,server,staged,visible,acceptedBytes,acceptedCount,
                         results,doneSent,duplicated,clientError,final,cancelResult>> /\ Record(3,0)
Process == /\ requests# <<>> /\ requests'=Tail(requests)
    /\ (LET m==Head(requests) IN
       CASE m.kind=1 ->
          LET bad==(m.seq=FailAt) \/ (failed /\ Bug#"forgetError")
              size==IF bad THEN 0 ELSE Size(m.seq)
          IN /\ failed'=(failed \/ bad) /\ server'=(IF bad THEN 3 ELSE server)
             /\ staged'=(IF bad THEN 0 ELSE staged+size)
             /\ acceptedBytes'=acceptedBytes+size /\ acceptedCount'=acceptedCount+(IF bad THEN 0 ELSE 1)
             /\ visible'=(IF Bug="earlyPublish" /\ ~bad THEN staged' ELSE visible)
             /\ results'=[results EXCEPT ![m.seq]=IF bad THEN 2 ELSE 1]
             /\ replies'=Append(replies,Msg(1,m.seq,results'[m.seq])) /\ Record(4,m.seq)
       [] m.kind=2 ->
          LET bad==failed \/ (staged#3 /\ Bug#"incomplete")
          IN /\ failed'=bad /\ server'=(IF bad THEN 3 ELSE 1) /\ staged'=0
             /\ visible'=(IF bad THEN 0 ELSE staged)
             /\ replies'=Append(replies,Msg(2,0,IF bad THEN 2 ELSE 1))
             /\ UNCHANGED <<acceptedBytes,acceptedCount,results>> /\ Record(5,0)
       [] OTHER ->
          /\ server'=(IF server=1 THEN 1 ELSE 2)
          /\ staged'=(IF Bug="cancelLeak" THEN staged ELSE 0)
          /\ replies'=Append(replies,Msg(3,0,IF server=1 THEN 4 ELSE 3))
          /\ UNCHANGED <<failed,visible,acceptedBytes,acceptedCount,results>> /\ Record(6,0))
    /\ UNCHANGED <<phase,sent,acked,credit,doneSent,cancelSent,duplicated,clientError,final,cancelResult>>
ReceiveAt(i) == /\ i\in 1..Len(replies) /\ (i=1 \/ ReorderReplies)
    /\ (replies[i].kind=1 \/ acked=sent)
    /\ replies'=SubSeq(replies,1,i-1)\o SubSeq(replies,i+1,Len(replies))
    /\ (LET m==replies[i] IN
       CASE m.kind=1 ->
          /\ credit'=(IF m.seq\notin acked \/ Bug="doubleCredit" THEN credit+Size(m.seq) ELSE credit)
          /\ acked'=acked\cup{m.seq} /\ clientError'=(clientError \/ m.result=2)
          /\ UNCHANGED <<phase,final,cancelResult>> /\ Record(7,m.seq)
       [] m.kind=2 ->
          /\ phase'=(IF phase=1 THEN 3 ELSE phase) /\ final'=m.result
          /\ clientError'=(clientError \/ m.result=2)
          /\ UNCHANGED <<credit,acked,cancelResult>> /\ Record(8,0)
       [] OTHER ->
          /\ phase'=(IF m.result=4 THEN 3 ELSE 4) /\ cancelResult'=m.result
          /\ UNCHANGED <<credit,acked,clientError,final>> /\ Record(9,0))
    /\ UNCHANGED <<sent,requests,failed,server,staged,visible,acceptedBytes,acceptedCount,results,doneSent,cancelSent,duplicated>>
Receive == \E i\in 1..Len(replies):ReceiveAt(i)
DuplicateAck == /\ ~duplicated /\ replies# <<>> /\ Head(replies).kind=1
                /\ replies'= <<Head(replies)>>\o replies /\ duplicated'=TRUE
                /\ UNCHANGED <<phase,sent,acked,credit,requests,failed,server,staged,visible,acceptedBytes,acceptedCount,
                               results,doneSent,cancelSent,clientError,final,cancelResult>> /\ Record(10,0)
Next == (Send \/ Done \/ Cancel \/ Process \/ Receive \/ DuplicateAck) /\ Project
Spec == Init /\ [][Next]_vars
TypeOK == /\ phase\in 0..4 /\ sent\subseteq Ids /\ acked\subseteq sent /\ credit\in Int
          /\ server\in 0..3 /\ staged\in 0..3 /\ visible\in 0..3 /\ acceptedBytes\in 0..3 /\ acceptedCount\in 0..2
          /\ results\in [Ids->0..2] /\ failed\in BOOLEAN /\ clientError\in BOOLEAN
          /\ doneSent\in BOOLEAN /\ cancelSent\in BOOLEAN /\ duplicated\in BOOLEAN
          /\ final\in 0..2 /\ cancelResult\in {0,3,4}
          /\ requests\in Seq([kind:1..3,seq:0..2,result:{0}])
          /\ replies\in Seq([kind:1..3,seq:0..2,result:1..4])
          /\ sentCount=Cardinality(sent) /\ ackCount=Cardinality(acked) /\ r1=results[1] /\ r2=results[2]
          /\ event\in 0..10 /\ item\in 0..2
WindowBound == credit\in 0..Window /\ Bytes(sent\acked)<=Window
CreditConservation == credit+Bytes(sent\acked)=Window
StickyErrors == \A n\in Ids:(results[n]#0 /\ FailAt\in Ids /\ n>=FailAt)=>results[n]=2
NoEarlyPublish == visible>0 => server=1
CompletionSound == server=1 => visible=3 /\ ~failed /\ acceptedCount=2
TerminalCleanup == server\in {1,2,3} => staged=0
ReceiptSound == /\ (final=1 => visible=3 /\ acked=sent)
                /\ (cancelResult=3 => server=2 /\ staged=0)
                /\ (cancelResult=4 => server=1 /\ visible=3)
=============================================================================
