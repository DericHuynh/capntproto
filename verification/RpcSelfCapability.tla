-------------------------- MODULE RpcSelfCapability --------------------------
EXTENDS Naturals, TLC
CONSTANTS Revocable, KeepWeakAlive, AllowBypass
VARIABLES root,alias,external,revoked,made,attempted,rejected,alive,available
owners == <<root,alias,external,revoked,made,attempted,rejected>>
vars == <<root,alias,external,revoked,made,attempted,rejected,alive,available>>
Live == (root \/ alias) /\ ~revoked
Init == /\ root=TRUE /\ alias=FALSE /\ external=TRUE /\ revoked=FALSE /\ made=FALSE
        /\ attempted=FALSE /\ rejected=FALSE /\ alive=TRUE /\ available=TRUE
Self == /\ Live /\ ~made /\ alias'=TRUE /\ made'=TRUE
        /\ UNCHANGED <<root,external,revoked,attempted,rejected>>
Construct == /\ Live /\ external /\ ~attempted /\ ~made
             /\ attempted'=TRUE /\ rejected'=(Revocable /\ ~AllowBypass)
             /\ alias'=~rejected' /\ made'=~rejected'
             /\ UNCHANGED <<root,external,revoked>>
DropRoot == /\ root /\ root'=FALSE /\ revoked'=(revoked \/ Revocable)
            /\ UNCHANGED <<alias,external,made,attempted,rejected>>
DropAlias == /\ alias /\ alias'=FALSE
             /\ UNCHANGED <<root,external,revoked,made,attempted,rejected>>
DropExternal == /\ external /\ external'=FALSE
                /\ UNCHANGED <<root,alias,revoked,made,attempted,rejected>>
Revoke == /\ Revocable /\ root /\ ~revoked /\ revoked'=TRUE
          /\ UNCHANGED <<root,alias,external,made,attempted,rejected>>
Next == /\ (Self \/ Construct \/ DropRoot \/ DropAlias \/ DropExternal \/ Revoke)
        /\ alive'=(external' \/ Live' \/ KeepWeakAlive)
        /\ available'=Live'
Spec == Init /\ [][Next]_vars
TypeOK == \A x\in {root,alias,external,revoked,made,attempted,rejected,alive,available}: x\in BOOLEAN
WeakOwnership == alive=(external \/ Live)
WeakAvailability == available=Live
NoBypass == (Revocable /\ attempted) => rejected
=============================================================================
