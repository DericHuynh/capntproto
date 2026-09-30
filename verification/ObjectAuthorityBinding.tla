------------------------ MODULE ObjectAuthorityBinding ------------------------
EXTENDS Naturals
CONSTANT Fault
\* Two object IDs/generations, one root, one delegation (VIEW/ALL), one ORM
\* binding and one get/put pair. Revocation may precede or follow each step.
\* The host is trusted to issue roots. IDs are not store brands. Generation is
\* preserved metadata; ORM itself does not maintain an incarnation registry.
VARIABLES stage, hostObject, hostGeneration, object, generation, requested,
          rights, live, target, read, write, usedLive, event
vars == <<stage,hostObject,hostGeneration,object,generation,requested,rights,
          live,target,read,write,usedLive,event>>
Init == /\ stage=0 /\ hostObject=0 /\ hostGeneration=0 /\ object=0 /\ generation=0
        /\ requested=0 /\ rights=1 /\ live=0 /\ target=0 /\ read=0 /\ write=0
        /\ usedLive=0 /\ event=0
Issue(o,g) == /\ stage=0 /\ stage'=1 /\ hostObject'=o /\ hostGeneration'=g
              /\ object'=o /\ generation'=g /\ live'=1 /\ event'=1
              /\ UNCHANGED <<requested,rights,target,read,write,usedLive>>
Delegate(r) == /\ stage=1 /\ live=1 /\ stage'=2 /\ requested'=r
               /\ rights'=(IF Fault="rights" THEN 1 ELSE r)
               /\ object'=(IF Fault="object" THEN 3-hostObject ELSE hostObject)
               /\ generation'=(IF Fault="generation" THEN 3-hostGeneration ELSE hostGeneration)
               /\ event'=2
               /\ UNCHANGED <<hostObject,hostGeneration,live,target,read,write,usedLive>>
Bind(t) == /\ stage=2 /\ target'=t /\ event'=3
           /\ stage'=(IF t=object \/ Fault="bind" THEN 3 ELSE 4)
           /\ UNCHANGED <<hostObject,hostGeneration,object,generation,requested,rights,live,read,write,usedLive>>
Use == /\ stage=3 /\ stage'=5 /\ usedLive'=live /\ event'=4
       /\ LET allowed == live=1 \/ Fault="revocation"
          IN /\ read'=(IF allowed THEN target ELSE 0)
             /\ write'=(IF allowed /\ rights=1 THEN target ELSE 0)
       /\ UNCHANGED <<hostObject,hostGeneration,object,generation,requested,rights,live,target>>
Revoke == /\ stage>0 /\ live=1 /\ live'=0 /\ event'=5
          /\ UNCHANGED <<stage,hostObject,hostGeneration,object,generation,requested,rights,target,read,write,usedLive>>
Idle == /\ stage>0 /\ event'=6
        /\ UNCHANGED <<stage,hostObject,hostGeneration,object,generation,requested,rights,live,target,read,write,usedLive>>
Next == (\E o\in 1..2,g\in 1..2: Issue(o,g)) \/ (\E r\in 0..1: Delegate(r))
        \/ (\E t\in 1..2: Bind(t)) \/ Use \/ Revoke \/ Idle
Spec == Init /\ [][Next]_vars
TypeOK == /\ stage\in 0..5 /\ hostObject\in 0..2 /\ hostGeneration\in 0..2
          /\ object\in 0..2 /\ generation\in 0..2 /\ requested\in 0..1 /\ rights\in 0..1
          /\ live\in 0..1 /\ target\in 0..2 /\ read\in 0..2 /\ write\in 0..2
          /\ usedLive\in 0..1 /\ event\in 0..6
GrantBindings == object=hostObject /\ generation=hostGeneration
Attenuation == stage>=2 => rights=requested
Binding == /\ (stage\in {3,5} => target=hostObject)
           /\ (stage=4 => target#hostObject)
Invocation == stage=5 =>
              /\ read=(IF usedLive=1 THEN hostObject ELSE 0)
              /\ write=(IF usedLive=1 /\ requested=1 THEN hostObject ELSE 0)
=============================================================================
