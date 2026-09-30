------------------------ MODULE RpcSchemaMetadata ------------------------
EXTENDS Naturals
CONSTANT Fault
\* One generic annotation application, a declaration arriving lazily, two
\* bound/value kinds (Text and Struct), and two independent schema authorities.
\* Three load/read/copy operations after setup; payload storage is immutable.
VARIABLES payload, bound, known, steps, event, foreign, accepted, writes
vars == <<payload,bound,known,steps,event,foreign,accepted,writes>>
Init == /\ payload=0 /\ bound=0 /\ known=0 /\ steps=0 /\ event=0
        /\ foreign=0 /\ accepted=0 /\ writes=0
Setup(p,b) == /\ payload=0 /\ payload'=p /\ bound'=b /\ event'=1
              /\ UNCHANGED <<known,steps,foreign,accepted,writes>>
Load(k) == /\ payload#0 /\ steps<3 /\ known=0 /\ known'=k
           /\ steps'=steps+1 /\ event'=2
           /\ UNCHANGED <<payload,bound,foreign,accepted,writes>>
Readable == (known=1 \/ Fault="missingDeclaration")
            /\ (payload=bound \/ Fault="wrongType")
Read == /\ payload#0 /\ steps<3 /\ steps'=steps+1 /\ event'=3
        /\ accepted'=IF Readable THEN 1 ELSE 0
        /\ UNCHANGED <<payload,bound,known,foreign,writes>>
Copy(f) == /\ payload#0 /\ steps<3 /\ steps'=steps+1 /\ event'=4
           /\ foreign'=f
           /\ LET allowed == Readable /\ (bound=1 \/ f=0 \/ Fault="foreignAggregate")
              IN /\ accepted'=IF allowed THEN 1 ELSE 0
                 /\ writes'=IF allowed THEN writes+1 ELSE writes
           /\ UNCHANGED <<payload,bound,known>>
Next == (\E p,b \in 1..2: Setup(p,b)) \/ (\E k \in 1..2: Load(k))
        \/ Read \/ (\E f \in 0..1: Copy(f))
Spec == Init /\ [][Next]_vars
TypeOK == /\ payload\in 0..2 /\ bound\in 0..2 /\ known\in 0..2
          /\ steps\in 0..3 /\ event\in 0..4 /\ foreign\in 0..1
          /\ accepted\in 0..1 /\ writes\in 0..3
DeclarationRequired == event\in {3,4} /\ accepted=1 => known=1
TypeAgreement == event\in {3,4} /\ accepted=1 => payload=bound
AggregateAuthority == event=4 /\ accepted=1 /\ bound=2 => foreign=0
=============================================================================
