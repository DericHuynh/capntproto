------------------------- MODULE RpcSchemaRetrieval -------------------------
EXTENDS Naturals, FiniteSets, TLC
CONSTANTS Scenario, Revision, WrongRevision, EarlyActivation
VARIABLES needed, asked, have, failed, active, published, tainted,
          event, item, wrong, needCount, askedCount, haveCount
vars == <<needed,asked,have,failed,active,published,tainted,event,item,wrong,needCount,askedCount,haveCount>>
Deps(k) == IF Scenario="shared" THEN (IF k=1 THEN {2,3} ELSE IF k=2 THEN {3} ELSE {})
           ELSE IF k=1 THEN {IF Scenario="missing" THEN 3 ELSE 2}
           ELSE IF Scenario="cycle" /\ k=2 THEN {1} ELSE {}
Available(k) == ~(Scenario="missing" /\ k=3)
Record(e,k,w) == /\ event'=e /\ item'=k /\ wrong'=w
                 /\ needCount'=Cardinality(needed') /\ askedCount'=Cardinality(asked') /\ haveCount'=Cardinality(have')
Init == /\ needed={1} /\ asked={} /\ have={} /\ failed=FALSE /\ active=FALSE /\ tainted=FALSE
        /\ published=Revision /\ event=0 /\ item=0 /\ wrong=FALSE
        /\ needCount=1 /\ askedCount=0 /\ haveCount=0
Ask(k) == /\ ~failed /\ ~active /\ k\in needed \ asked /\ (\A j\in needed \ asked:k<=j)
          /\ asked'=asked\cup{k} /\ UNCHANGED <<needed,have,failed,active,published,tainted>> /\ Record(1,k,FALSE)
Receive(k,bad) == /\ ~failed /\ ~active /\ k\in asked \ have
    /\ (IF Available(k) /\ (~bad \/ WrongRevision)
       THEN /\ have'=have\cup{k} /\ needed'=needed\cup Deps(k) /\ tainted'=(tainted \/ bad) /\ UNCHANGED failed
       ELSE /\ failed'=TRUE /\ UNCHANGED <<have,needed,tainted>>)
    /\ UNCHANGED <<asked,active,published>> /\ Record(2,k,bad)
Activate == /\ ~failed /\ ~active /\ 1\in have /\ (needed\subseteq have \/ EarlyActivation)
            /\ active'=TRUE /\ UNCHANGED <<needed,asked,have,failed,published,tainted>> /\ Record(3,0,FALSE)
Publish == /\ published=1 /\ published'=2 /\ UNCHANGED <<needed,asked,have,failed,active,tainted>> /\ Record(4,0,FALSE)
Cancel == /\ ~active /\ ~failed /\ failed'=TRUE /\ needed'={} /\ asked'={} /\ have'={}
          /\ UNCHANGED <<active,published,tainted>> /\ Record(5,0,FALSE)
Next == (\E k\in 1..3:Ask(k) \/ (\E bad\in BOOLEAN:Receive(k,bad))) \/ Activate \/ Publish \/ Cancel
Spec == Init /\ [][Next]_vars
TypeOK == /\ needed\subseteq 1..3 /\ asked\subseteq needed /\ have\subseteq asked
          /\ failed\in BOOLEAN /\ active\in BOOLEAN /\ tainted\in BOOLEAN /\ published\in {1,2}
          /\ event\in 0..5 /\ item\in 0..3 /\ wrong\in BOOLEAN
          /\ needCount=Cardinality(needed) /\ askedCount=Cardinality(asked) /\ haveCount=Cardinality(have)
SchemaIntegrity == ~tainted
Closure == active => (needed\subseteq have /\ \A k\in have:Deps(k)\subseteq have)
NoRejectedUse == active => ~failed
=============================================================================
