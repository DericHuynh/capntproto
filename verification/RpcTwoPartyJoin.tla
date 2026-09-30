------------------------- MODULE RpcTwoPartyJoin -------------------------
EXTENDS Naturals, TLC
CONSTANTS Remote, Equal, Fault
VARIABLES received, finished, resolved, downstream, forwarded, outcome,
          r0, r1, caps, held, canceled, connected, reused, reuseSafe, needsFinish
vars == <<received,finished,resolved,downstream,forwarded,outcome,r0,r1,caps,held,canceled,connected,reused,reuseSafe,needsFinish>>
Has(mask,i) == (mask \div (i+1)) % 2 = 1
Add(mask,i) == mask+i+1
Bit(b) == IF b THEN 1 ELSE 0
Init == /\ received=0 /\ finished=0 /\ resolved=FALSE /\ downstream=0 /\ forwarded=FALSE
        /\ outcome=0 /\ r0=0 /\ r1=0 /\ caps=0 /\ held=FALSE /\ canceled=FALSE
        /\ connected=TRUE /\ reused=FALSE /\ reuseSafe=TRUE /\ needsFinish=TRUE
\* One action plus drained runtime work. Codes: outcome 0=pending,1=equal,
\* 2=unequal,3=canceled; replies 0=none,1/2=results,3=canceled,4=exception.
Advance(rec,fin,ready,down,conn) ==
    /\ received'=rec /\ finished'=fin /\ resolved'=ready /\ downstream'=down /\ connected'=conn
    /\ canceled'=(canceled \/ (outcome=0 /\ fin#finished))
    /\ forwarded'=(forwarded \/ (Remote /\ (rec=3 \/ (Fault="collectEarly" /\ rec#0)) /\ ~canceled'))
    /\ LET complete == conn /\ ~canceled' /\ (rec=3 \/ (Fault="replyEarly" /\ rec#0))
                       /\ (IF Remote THEN down=3 ELSE ready)
           result == IF Equal \/ Fault="falseEquality" THEN 1 ELSE 2
       IN /\ outcome'=(IF canceled' THEN (IF Fault="loseCancel" THEN outcome ELSE 3)
                       ELSE IF outcome#0 THEN outcome ELSE IF complete THEN result ELSE 0)
          /\ r0'=(IF r0#0 \/ ~conn THEN r0 ELSE IF Has(fin,0) THEN 3
                   ELSE IF Has(rec,0) /\ canceled' THEN 4 ELSE IF complete /\ Has(rec,0) THEN result ELSE 0)
          /\ r1'=(IF r1#0 \/ ~conn THEN r1 ELSE IF Has(fin,1) THEN 3
                   ELSE IF Has(rec,1) /\ canceled' THEN 4 ELSE IF complete /\ Has(rec,1) THEN result ELSE 0)
    /\ caps'=Bit(r0'=1)+(IF Fault="twoCaps" THEN Bit(r1'=1) ELSE 0)
    /\ held'=((Remote /\ forwarded' /\ conn /\ ~canceled' /\ fin#3 /\ ~(Fault="releaseEarly" /\ outcome'\in {1,2}))
              \/ (Fault="leakHold" /\ held))
    /\ needsFinish'=(Fault#"omitFinish" \/ outcome'\notin {1,2})
    /\ UNCHANGED <<reused,reuseSafe>>
Receive(i) == /\ connected /\ outcome=0 /\ ~Has(received,i)
              /\ Advance(Add(received,i),finished,resolved,downstream,connected)
Resolve == /\ connected /\ ~Remote /\ ~resolved
           /\ Advance(received,finished,TRUE,downstream,connected)
Reply(i) == /\ connected /\ Remote /\ forwarded /\ ~Has(downstream,i)
            /\ Advance(received,finished,resolved,Add(downstream,i),connected)
Finish(i) == /\ connected /\ Has(received,i) /\ ~Has(finished,i)
             /\ Advance(received,Add(finished,i),resolved,downstream,connected)
Disconnect == /\ connected /\ Advance(received,finished,resolved,downstream,FALSE)
Reuse == /\ connected /\ ~reused /\ received#0 /\ (finished=received \/ Fault="reuseLive")
         /\ (~forwarded \/ downstream=3)
         /\ reused'=TRUE /\ reuseSafe'=(finished=received)
         /\ received'=0 /\ finished'=0 /\ downstream'=0 /\ forwarded'=FALSE
         /\ outcome'=0 /\ r0'=0 /\ r1'=0 /\ caps'=0 /\ held'=FALSE /\ canceled'=FALSE /\ needsFinish'=TRUE
         /\ UNCHANGED <<resolved,connected>>
Next == (\E i\in 0..1 : Receive(i) \/ Reply(i) \/ Finish(i)) \/ Resolve \/ Disconnect \/ Reuse
Spec == Init /\ [][Next]_vars
TypeOK == /\ received\in 0..3 /\ finished\in 0..3 /\ downstream\in 0..3 /\ outcome\in 0..3
          /\ r0\in 0..4 /\ r1\in 0..4 /\ caps\in 0..2
          /\ \A b\in {resolved,forwarded,held,canceled,connected,reused,reuseSafe,needsFinish}:b\in BOOLEAN
CollectBeforeForward == forwarded => received=3
NoEarlyReply == outcome\in {1,2} => received=3 /\ (IF Remote THEN downstream=3 ELSE resolved)
Equality == /\ (outcome=1 => Equal) /\ (outcome=2 => ~Equal)
SingleCapability == caps=Bit(r0=1)
Cancellation == canceled => outcome=3
Retention == /\ held=(Remote /\ forwarded /\ connected /\ ~canceled /\ finished#3)
             /\ (outcome\in {1,2} => needsFinish)
NoPrematureReuse == reuseSafe
=============================================================================
