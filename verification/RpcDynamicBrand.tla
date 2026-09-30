------------------------- MODULE RpcDynamicBrand -------------------------
EXTENDS Naturals, TLC
CONSTANTS Compatible, Empty, IgnoreBrand, DamageOnError
VARIABLES attempted, accepted, slot, root, message, held, read, used, result, alive1, alive2, cleared, prior
vars == <<attempted,accepted,slot,root,message,held,read,used,result,alive1,alive2,cleared,prior>>
Alive(id) == root \/ (message /\ slot=id) \/ held=id
Init == /\ attempted=FALSE /\ accepted=FALSE /\ slot=IF Empty THEN 0 ELSE 1
        /\ root=TRUE /\ message=TRUE /\ held=0 /\ read=FALSE /\ used=FALSE
        /\ result=0 /\ alive1=TRUE /\ alive2=TRUE /\ cleared=FALSE /\ prior=slot
Assign == /\ root /\ message /\ ~attempted /\ attempted'=TRUE
          /\ accepted'=(Compatible \/ IgnoreBrand) /\ prior'=slot /\ cleared'=FALSE
          /\ slot'=IF accepted' THEN 2 ELSE IF DamageOnError THEN 0 ELSE slot
          /\ UNCHANGED <<root,message,held,read,used,result>>
Read == /\ message /\ slot#0 /\ ~read /\ read'=TRUE /\ held'=slot
        /\ UNCHANGED <<attempted,accepted,slot,root,message,used,result,cleared,prior>>
DropRoot == /\ root /\ root'=FALSE
            /\ UNCHANGED <<attempted,accepted,slot,message,held,read,used,result,cleared,prior>>
DropMessage == /\ message /\ message'=FALSE
               /\ UNCHANGED <<attempted,accepted,slot,root,held,read,used,result,cleared,prior>>
DropHeld == /\ held#0 /\ held'=0
            /\ UNCHANGED <<attempted,accepted,slot,root,message,read,used,result,cleared,prior>>
Use == /\ held#0 /\ ~used /\ used'=TRUE /\ result'=held
       /\ UNCHANGED <<attempted,accepted,slot,root,message,held,read,cleared,prior>>
Clear == /\ message /\ slot#0 /\ slot'=0 /\ cleared'=TRUE
         /\ UNCHANGED <<attempted,accepted,root,message,held,read,used,result,prior>>
Next == /\ (Assign \/ Read \/ DropRoot \/ DropMessage \/ DropHeld \/ Use \/ Clear)
        /\ alive1'=Alive(1)' /\ alive2'=Alive(2)'
Spec == Init /\ [][Next]_vars
TypeOK == /\ \A x\in {attempted,accepted,root,message,read,used,alive1,alive2,cleared}:x\in BOOLEAN
          /\ slot\in 0..2 /\ held\in 0..2 /\ result\in 0..2 /\ prior\in 0..2
BrandCheck == attempted => accepted=Compatible
FailedAssignment == (attempted /\ ~Compatible /\ ~cleared) => slot=prior
Ownership == /\ alive1=Alive(1) /\ alive2=Alive(2)
ResultIdentity == used => result\in 1..2
=============================================================================
