------------------------ MODULE CapnpBulkTransfer ------------------------
EXTENDS Integers, Sequences, FiniteSets, TLC
\* Fixed byte window over ordinary streaming Calls and Returns. Local send
\* admission is not remote completion. done() observes sticky stream errors.
\* Explicit cancel is an application-level operation, not a new RPC message.
CONSTANTS ChunkCount, Window, FailAt, AllowCancel, AllowDuplicateAck, Bug
ASSUME /\ ChunkCount \in Nat \ {0}
       /\ Window \in Nat /\ Window >= (IF ChunkCount>=2 THEN 2 ELSE 1)
       /\ FailAt \in 0..ChunkCount
       /\ AllowCancel \in BOOLEAN /\ AllowDuplicateAck \in BOOLEAN
       /\ Bug \in {"none","ignoreWindow","doubleCredit","forgetError"}
Chunks == 1..ChunkCount
Size(n) == IF n % 2=0 THEN 2 ELSE 1
RECURSIVE Bytes(_)
Bytes(xs) == IF xs={} THEN 0 ELSE LET n==CHOOSE x \in xs:TRUE IN Size(n)+Bytes(xs\{n})
Msg(kind,n,result) == [kind |-> kind, chunk |-> n, result |-> result]
VARIABLES phase, sent, acked, credit, requests, replies, executed, results,
          serverError, final, duplicated
vars == <<phase,sent,acked,credit,requests,replies,executed,results,serverError,final,duplicated>>
Init == /\ phase="sending" /\ sent={} /\ acked={} /\ credit=Window
        /\ requests= <<>> /\ replies= <<>> /\ executed= <<>>
        /\ results=[n \in Chunks |-> "none"] /\ serverError=FALSE
        /\ final="none" /\ duplicated=FALSE
Send == /\ phase="sending" /\ Cardinality(sent)<ChunkCount
        /\ LET n==Cardinality(sent)+1
           IN /\ (credit>=Size(n) \/ Bug="ignoreWindow")
              /\ credit'=credit-Size(n) /\ sent'=sent \cup {n}
              /\ requests'=Append(requests,Msg("chunk",n,"none"))
        /\ UNCHANGED <<phase,acked,replies,executed,results,serverError,final,duplicated>>
SendDone == /\ phase="sending" /\ sent=Chunks /\ phase'="finishing"
            /\ requests'=Append(requests,Msg("done",0,"none"))
            /\ UNCHANGED <<sent,acked,credit,replies,executed,results,serverError,final,duplicated>>
Cancel == /\ AllowCancel /\ phase="sending" /\ phase'="canceling"
          /\ requests'=Append(requests,Msg("cancel",0,"none"))
          /\ UNCHANGED <<sent,acked,credit,replies,executed,results,serverError,final,duplicated>>
Process ==
    /\ requests # <<>> /\ requests'=Tail(requests)
    /\ LET m==Head(requests)
       IN CASE m.kind="chunk" ->
            LET failed==(m.chunk=FailAt) \/ (serverError /\ Bug # "forgetError")
                result==IF failed THEN "error" ELSE "ok"
            IN /\ executed'=Append(executed,m.chunk)
               /\ results'=[results EXCEPT ![m.chunk]=result]
               /\ serverError'=(serverError \/ failed)
               /\ replies'=Append(replies,Msg("ack",m.chunk,result))
          [] m.kind="done" ->
               /\ replies'=Append(replies,Msg("done",0,IF serverError THEN "error" ELSE "ok"))
               /\ UNCHANGED <<executed,results,serverError>>
          [] OTHER -> /\ replies'=Append(replies,Msg("cancel",0,"canceled"))
                      /\ UNCHANGED <<executed,results,serverError>>
    /\ UNCHANGED <<phase,sent,acked,credit,final,duplicated>>
Receive ==
    /\ replies # <<>> /\ replies'=Tail(replies)
    /\ LET m==Head(replies)
       IN CASE m.kind="ack" ->
                /\ credit'=IF m.chunk \notin acked \/ Bug="doubleCredit"
                            THEN credit+Size(m.chunk) ELSE credit
                /\ acked'=acked \cup {m.chunk} /\ UNCHANGED <<phase,final>>
          [] m.kind="done" -> /\ phase'="done" /\ final'=m.result
                              /\ UNCHANGED <<credit,acked>>
          [] OTHER -> /\ phase'="canceled" /\ final'="canceled"
                      /\ UNCHANGED <<credit,acked>>
    /\ UNCHANGED <<sent,requests,executed,results,serverError,duplicated>>
DuplicateAck == /\ AllowDuplicateAck /\ ~duplicated /\ replies # <<>>
                /\ Head(replies).kind="ack" /\ duplicated'=TRUE
                /\ replies'= <<Head(replies)>> \o replies
                /\ UNCHANGED <<phase,sent,acked,credit,requests,executed,results,serverError,final>>
Next == Send \/ SendDone \/ Cancel \/ Process \/ Receive \/ DuplicateAck
Spec == Init /\ [][Next]_vars
LiveSpec == Spec /\ WF_vars(Send) /\ WF_vars(SendDone) /\ WF_vars(Process) /\ WF_vars(Receive)
TypeOK == /\ phase \in {"sending","finishing","canceling","done","canceled"}
          /\ sent \subseteq Chunks /\ acked \subseteq sent /\ credit \in Int
          /\ executed \in Seq(Chunks) /\ results \in [Chunks -> {"none","ok","error"}]
          /\ serverError \in BOOLEAN /\ duplicated \in BOOLEAN
          /\ final \in {"none","ok","error","canceled"}
          /\ requests \in Seq([kind:{"chunk","done","cancel"},chunk:0..ChunkCount,result:{"none"}])
          /\ replies \in Seq([kind:{"ack","done","cancel"},chunk:0..ChunkCount,result:{"ok","error","canceled"}])
WindowBound == credit \in 0..Window /\ Bytes(sent\acked)<=Window
CreditConservation == credit+Bytes(sent\acked)=Window
InOrderExactlyOnce == executed=[i \in 1..Len(executed) |-> i]
StickyErrors == \A n \in Chunks : (results[n] # "none" /\ FailAt \in Chunks /\ n>=FailAt) => results[n]="error"
CompletionSound == phase="done" =>
    /\ Len(executed)=ChunkCount /\ acked=Chunks
    /\ final=IF FailAt \in Chunks THEN "error" ELSE "ok"
TerminalCleanup == phase \in {"done","canceled"} =>
    requests= <<>> /\ replies= <<>> /\ acked=sent /\ credit=Window
Settles == <>(phase \in {"done","canceled"})
NoBulkWitness == phase # "done"
NoCancelWitness == phase # "canceled"
NoBackpressureWitness == ~(credit=0 /\ Cardinality(sent)<ChunkCount)
NoErrorWitness == final # "error"
=============================================================================
