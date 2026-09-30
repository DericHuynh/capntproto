------------------------- MODULE RpcSchemaUpgrade -------------------------
EXTENDS Naturals, TLC
CONSTANTS ForgetConstraint, PublishRejected
VARIABLES parent, element, required, failed, previousParent, previousElement, event, steps
vars == <<parent,element,required,failed,previousParent,previousElement,event,steps>>
Init == /\ parent=0 /\ element=0 /\ required=FALSE /\ failed=FALSE
        /\ previousParent=0 /\ previousElement=0 /\ event=0 /\ steps=0
Record(e) == /\ previousParent'=parent /\ previousElement'=element /\ event'=e /\ steps'=steps+1
LoadParent(version) ==
    /\ steps<4
    /\ LET need == required \/ (parent#0 /\ parent#version)
           reject == need /\ element=3
       IN /\ failed'=reject
          /\ parent'=IF reject /\ ~PublishRejected THEN parent ELSE IF parent>version THEN parent ELSE version
          /\ element'=IF reject THEN element ELSE IF element=0 /\ (version=2 \/ need) THEN 1 ELSE element
          /\ required'=IF reject THEN required ELSE need
    /\ Record(version)
LoadElement(value) ==
    /\ steps<4
    /\ LET reject == ((required /\ ~ForgetConstraint) /\ value=3) \/ (element\in {2,3} /\ element#value)
       IN /\ failed'=reject /\ element'=IF reject THEN element ELSE value
    /\ UNCHANGED <<parent,required>> /\ Record(value+1)
Next == (\E v\in {1,2}:LoadParent(v)) \/ (\E v\in {2,3}:LoadElement(v))
Spec == Init /\ [][Next]_vars
TypeOK == /\ parent\in 0..2 /\ element\in 0..3 /\ required\in BOOLEAN /\ failed\in BOOLEAN
          /\ previousParent\in 0..2 /\ previousElement\in 0..3 /\ event\in 0..4 /\ steps\in 0..4
AtomicFailure == failed => (parent=previousParent /\ element=previousElement)
UpgradeConstraint == required => element#3
DependencyClosure == parent=2 => element#0
=============================================================================
