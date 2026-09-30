----------------------- MODULE PersistentFactoryBinding -----------------------
EXTENDS Naturals
CONSTANT Fault
\* Two registered kinds, two object IDs and two generations. Kind k's factory
\* accepts generation k. Rights 0/1 mean VIEW+DELEGATE/ALL. One descriptor is bound and
\* saved per history, followed by at most one restoration. Owner authorization,
\* revocation, restart and expiry are checked by the separate persistence models.
VARIABLES registered, stage, kind, object, generation, rights,
          outcome, observedObject, mayWrite, event
vars == <<registered,stage,kind,object,generation,rights,outcome,observedObject,mayWrite,event>>
Has(k) == (registered \div 2^(k-1)) % 2 = 1
Init == /\ registered=0 /\ stage=0 /\ kind=0 /\ object=0 /\ generation=0 /\ rights=0
        /\ outcome=0 /\ observedObject=0 /\ mayWrite=0 /\ event=0
Register(k) == /\ ~Has(k) /\ registered'=registered+2^(k-1) /\ event'=k
               /\ UNCHANGED <<stage,kind,object,generation,rights,outcome,observedObject,mayWrite>>
Bind(k,o,g,r) == /\ stage=0 /\ kind'=k /\ object'=o /\ generation'=g /\ rights'=r
                 /\ stage'=(IF Has(k) \/ Fault="registration" THEN 1 ELSE 3)
                 /\ event'=3
                 /\ UNCHANGED <<registered,outcome,observedObject,mayWrite>>
Restore == /\ stage=1 /\ stage'=2 /\ event'=4
           /\ LET selected == IF Fault="dispatch" THEN 3-kind ELSE kind
                  accepted == generation=selected \/ Fault="generation"
              IN /\ outcome'=(IF accepted THEN 1 ELSE 2)
                 /\ observedObject'=(IF ~accepted THEN 0 ELSE IF Fault="object" THEN 3-object ELSE object)
                 /\ mayWrite'=(IF ~accepted THEN 0 ELSE IF Fault="rights" THEN 1 ELSE rights)
           /\ UNCHANGED <<registered,kind,object,generation,rights>>
Idle == /\ stage\in {2,3} /\ event'=5
        /\ UNCHANGED <<registered,stage,kind,object,generation,rights,outcome,observedObject,mayWrite>>
Next == (\E k\in 1..2: Register(k))
        \/ (\E k\in 1..2,o\in 1..2,g\in 1..2,r\in 0..1: Bind(k,o,g,r))
        \/ Restore \/ Idle
Spec == Init /\ [][Next]_vars
TypeOK == /\ registered\in 0..3 /\ stage\in 0..3 /\ kind\in 0..2 /\ object\in 0..2
          /\ generation\in 0..2 /\ rights\in 0..1 /\ outcome\in 0..2
          /\ observedObject\in 0..2 /\ mayWrite\in 0..1 /\ event\in 0..5
RegisteredBinding == stage\in {1,2} => Has(kind)
RestorationContract == stage=2 => outcome=(IF generation=kind THEN 1 ELSE 2)
ExactObject == outcome=1 => observedObject=object
ExactRights == outcome=1 => mayWrite=rights
RejectedHasNoAuthority == outcome=2 => observedObject=0 /\ mayWrite=0
=============================================================================
