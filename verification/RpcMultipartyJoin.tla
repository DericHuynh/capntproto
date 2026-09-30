----------------------- MODULE RpcMultipartyJoin -----------------------
EXTENDS Naturals, FiniteSets, TLC
CONSTANTS NParts, Scenario, Fault
Parts == 0..(NParts-1)
Full == 2^NParts-1
Has(mask,p) == (mask \div (2^p)) % 2 = 1
Members(mask) == {p \in Parts : Has(mask,p)}
Count(mask) == Cardinality(Members(mask))
GroupCount(mask) == IF mask=0 THEN 0
  ELSE IF Scenario \in {"equal","tampered"} THEN 1
  ELSE (IF Has(mask,NParts-1) THEN 1 ELSE 0)
       + (IF Members(mask) \ {NParts-1} # {} THEN 1 ELSE 0)
VARIABLES arrived, released, phase, held, groups, accepted, canceled, event, part
vars == <<arrived,released,phase,held,groups,accepted,canceled,event,part>>
\* Phase 0 collecting, 1 proof checked, 2 acquired, 3 conflicting endpoints,
\* 4 canceled, 5 invalid proof. Shares and MAC security are idealized here.
Init == /\ arrived=0 /\ released=0 /\ phase=0 /\ held=0 /\ groups=0
        /\ accepted=0 /\ canceled=FALSE /\ event=0 /\ part=0
Update(a,r,s,c,e,p) ==
    /\ arrived'=a /\ released'=r /\ phase'=s /\ canceled'=c /\ event'=e /\ part'=p
    /\ held'=(IF Fault="leakGuard" /\ e=5 THEN held ELSE Count(a-r))
    /\ groups'=GroupCount(a-r)
    /\ accepted'=(IF e=3 THEN accepted+1 ELSE accepted)
Arrive(p) == /\ phase=0 /\ ~Has(arrived,p)
             /\ Update(arrived+2^p,released,phase,canceled,1,p)
Decide == /\ phase=0 /\ (arrived=Full \/ (Fault="missingShare" /\ arrived#0))
          /\ LET same == Scenario="equal" \/ (Scenario="object" /\ Fault="mergeObjects")
                         \/ (Scenario="host" /\ Fault="ignoreHost")
                         \/ (Scenario="tampered" /\ Fault="ignoreProof")
                 s == IF same THEN 1 ELSE IF Scenario="tampered" THEN 5 ELSE 3
             IN Update(arrived,released,s,canceled,2,0)
Accept == /\ phase=1 \/ (Fault="acceptCanceled" /\ phase=4 /\ arrived=Full)
          /\ Update(arrived,released,2,canceled,3,0)
Cancel == /\ phase \in {0,1} /\ Update(arrived,released,4,TRUE,4,0)
Finish(p) == /\ phase \in {2,3,4,5} \/ (Fault="earlyRelease" /\ phase=1)
             /\ Has(arrived,p) /\ ~Has(released,p)
             /\ Update(arrived,released+2^p,phase,canceled,5,p)
Next == (\E p \in Parts : Arrive(p) \/ Finish(p)) \/ Decide \/ Accept \/ Cancel
Spec == Init /\ [][Next]_vars
TypeOK == /\ arrived \in 0..Full /\ released \in 0..Full /\ phase \in 0..5
          /\ held \in 0..NParts /\ groups \in 0..NParts /\ accepted \in 0..1
          /\ canceled \in BOOLEAN /\ event \in 0..5 /\ part \in Parts
CompleteProof == phase=1 => arrived=Full
Equality == accepted=1 => Scenario="equal"
NoRevival == canceled => accepted=0
Retention == phase \in {0,1} => released=0
Cleanup == /\ held=Count(arrived-released) /\ groups=GroupCount(arrived-released)
NoUnknownFinish == Members(released) \subseteq Members(arrived)
LiveSpec == Spec /\ WF_vars(Decide) /\ WF_vars(Accept)
                 /\ (\A p\in Parts : WF_vars(Arrive(p)) /\ WF_vars(Finish(p)))
Settles == <>(phase \in {2,3,4,5} /\ held=0)
=============================================================================
