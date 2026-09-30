----------------------- MODULE CapnpSchemaExchange -----------------------
EXTENDS Naturals, FiniteSets, TLC
\* Proposed application-level schema-fetch contract, NOT a new rpc.capnp tag.
\* Keys are <<schema ID, immutable revision>>. Revision pinning and strict
\* dependency closure are design choices, not SchemaLoader's stub policy.
CONSTANTS Revision, Scenario, AllowCorruption, Bug
ASSUME /\ Revision \in {1,2}
       /\ Scenario \in {"normal","cycle","missing","shared"}
       /\ AllowCorruption \in BOOLEAN
       /\ Bug \in {"none","wrongRevision","earlyActivation"}
Keys == {1,2,3} \X {1,2}
Root == <<1,Revision>>
Available == (IF Scenario="missing" THEN {1,2} ELSE {1,2,3}) \X {1,2}
Deps(k) == IF Scenario="shared" THEN
               IF k[1]=1 THEN {<<2,k[2]>>,<<3,k[2]>>}
               ELSE IF k[1]=2 THEN {<<3,k[2]>>} ELSE {}
           ELSE IF k[1]=1 THEN {<<IF Scenario="missing" THEN 3 ELSE 2,k[2]>>}
           ELSE IF k[1]=2 /\ Scenario="cycle" THEN {<<1,k[2]>>} ELSE {}
Required == {Root} \cup Deps(Root)
Reply(k,body,found) == [key |-> k, body |-> body, found |-> found]
VARIABLES published, needed, requested, requests, replies, cache, rejected, phase
vars == <<published,needed,requested,requests,replies,cache,rejected,phase>>
Init == /\ published=Revision /\ needed={Root} /\ requested={} /\ requests={}
        /\ replies={} /\ cache=[k \in Keys |-> <<0,0>>]
        /\ rejected={} /\ phase="fetching"
Publish == /\ published=1 /\ published'=2
           /\ UNCHANGED <<needed,requested,requests,replies,cache,rejected,phase>>
Ask(k) == /\ phase="fetching" /\ k \in needed \ requested
          /\ requested'=requested \cup {k} /\ requests'=requests \cup {k}
          /\ UNCHANGED <<published,needed,replies,cache,rejected,phase>>
Serve(k,corrupt) ==
    /\ k \in requests /\ (~corrupt \/ AllowCorruption)
    /\ LET body == IF corrupt THEN <<k[1],3-k[2]>> ELSE k
       IN replies'=replies \cup {Reply(k,body,k \in Available /\ k[2]<=published)}
    /\ requests'=requests \ {k}
    /\ UNCHANGED <<published,needed,requested,cache,rejected,phase>>
Receive(m) ==
    /\ m \in replies /\ replies'=replies \ {m}
    /\ IF phase # "fetching" THEN UNCHANGED <<cache,needed,rejected>>
       ELSE IF m.found /\ (m.body=m.key \/ Bug="wrongRevision")
            THEN /\ cache'=[cache EXCEPT ![m.key]=m.body]
                 /\ needed'=needed \cup Deps(m.body) /\ UNCHANGED rejected
            ELSE /\ rejected'=rejected \cup {m.key} /\ UNCHANGED <<cache,needed>>
    /\ UNCHANGED <<published,requested,requests,phase>>
Activate == /\ phase="fetching" /\ cache[Root] # <<0,0>>
            /\ (Bug="earlyActivation" \/ \A k \in needed : cache[k] # <<0,0>>)
            /\ phase'="ready"
            /\ UNCHANGED <<published,needed,requested,requests,replies,cache,rejected>>
Fail == /\ phase="fetching" /\ rejected # {} /\ phase'="failed"
        /\ UNCHANGED <<published,needed,requested,requests,replies,cache,rejected>>
Next == Publish \/ (\E k \in Keys : Ask(k) \/ (\E corrupt \in BOOLEAN : Serve(k,corrupt)))
        \/ (\E m \in replies : Receive(m)) \/ Activate \/ Fail
Spec == Init /\ [][Next]_vars
LiveSpec == Spec /\ WF_vars(Activate) /\ WF_vars(Fail)
            /\ (\A k \in Keys : WF_vars(Ask(k)) /\ WF_vars(Serve(k,FALSE)))
            /\ (\A m \in [key:Keys,body:Keys,found:BOOLEAN] : WF_vars(Receive(m)))
TypeOK == /\ published \in {1,2} /\ needed \subseteq Keys /\ requested \subseteq Keys
          /\ requests \subseteq requested /\ rejected \subseteq requested
          /\ cache \in [Keys -> Keys \cup {<<0,0>>}]
          /\ replies \subseteq [key:Keys,body:Keys,found:BOOLEAN]
          /\ phase \in {"fetching","ready","failed"}
SchemaIntegrity == \A k \in Keys : cache[k] # <<0,0>> => cache[k]=k
DependencyClosure == phase="ready" =>
    /\ \A k \in Required : cache[k]=k
    /\ \A k \in Keys : cache[k] # <<0,0>> => \A d \in Deps(k) : cache[d]=d
PinnedRevision == phase="ready" => \A k \in Required : cache[k][2]=Revision
NoRejectedUse == phase="ready" => rejected={}
Settles == <>(phase \in {"ready","failed"})
SuccessfulFetch == <>(phase="ready")
MissingFails == <>(phase="failed")
NoSchemaWitness == phase # "ready"
NoCycleWitness == ~(Scenario="cycle" /\ phase="ready")
NoRejectionWitness == rejected={}
NoPinnedUpgradeWitness == ~(published=2 /\ Revision=1 /\ phase="ready")
=============================================================================
