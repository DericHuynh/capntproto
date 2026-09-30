-------------------------- MODULE RpcDynamicOwnership --------------------------
EXTENDS Naturals, TLC
CONSTANTS Revocable, LoseReadOwnership, LoseRevocation
VARIABLES root,message,value,copy,read,copied,revoked,alive,usable
vars == <<root,message,value,copy,read,copied,revoked,alive,usable>>
Owners == root \/ message \/ value \/ copy
Alive == IF Revocable THEN root /\ ~revoked ELSE Owners
Init == /\ root=TRUE /\ message=TRUE /\ value=FALSE /\ copy=FALSE
        /\ read=FALSE /\ copied=FALSE /\ revoked=FALSE /\ alive=TRUE /\ usable=TRUE
Read == /\ message /\ ~read /\ value'=TRUE /\ read'=TRUE
        /\ UNCHANGED <<root,message,copy,copied,revoked>>
Clone == /\ value /\ ~copied /\ copy'=TRUE /\ copied'=TRUE
         /\ UNCHANGED <<root,message,value,read,revoked>>
DropRoot == /\ root /\ root'=FALSE /\ revoked'=(revoked \/ Revocable)
            /\ UNCHANGED <<message,value,copy,read,copied>>
DropMessage == /\ message /\ message'=FALSE
               /\ UNCHANGED <<root,value,copy,read,copied,revoked>>
DropValue == /\ value /\ value'=FALSE
             /\ UNCHANGED <<root,message,copy,read,copied,revoked>>
DropCopy == /\ copy /\ copy'=FALSE
            /\ UNCHANGED <<root,message,value,read,copied,revoked>>
Revoke == /\ Revocable /\ root /\ ~revoked /\ revoked'=TRUE
          /\ UNCHANGED <<root,message,value,copy,read,copied>>
Next == /\ (Read \/ Clone \/ DropRoot \/ DropMessage \/ DropValue \/ DropCopy \/ Revoke)
        /\ alive'=(IF ~Revocable /\ LoseReadOwnership THEN root' \/ message' ELSE Alive')
        /\ usable'=(IF LoseRevocation /\ (value' \/ copy') THEN TRUE ELSE Alive' /\ ~revoked')
Spec == Init /\ [][Next]_vars
TypeOK == \A x\in {root,message,value,copy,read,copied,revoked,alive,usable}: x\in BOOLEAN
OwnedReads == alive=Alive
NoAuthorityRevival == usable=(Alive /\ ~revoked)
=============================================================================
