------------------------- MODULE RpcDynamicCapability -------------------------
EXTENDS Naturals, TLC
CONSTANTS Allow, Early, Fail, UseSchemaCancellation, LoseReadReference
VARIABLES caller,completed,running,alive,held,extracted,used
vars == <<caller,completed,running,alive,held,extracted,used>>
\* A reflected pending call has dispatched. The declaring schema allows
\* cancellation, but the dynamic server's explicit option is authoritative.
Init == /\ caller=TRUE /\ completed=FALSE /\ running=TRUE /\ alive=TRUE
        /\ held=FALSE /\ extracted=FALSE /\ used=FALSE
DropCaller == /\ caller /\ caller'=FALSE
              /\ running'=(running /\ ~(Allow \/ UseSchemaCancellation))
              /\ alive'=(running' \/ (held /\ ~LoseReadReference))
              /\ UNCHANGED <<completed,held,extracted,used>>
Complete == /\ running /\ running'=FALSE /\ completed'=TRUE
            /\ alive'=(caller /\ ~Fail)
            /\ UNCHANGED <<caller,held,extracted,used>>
Extract == /\ caller /\ completed /\ ~Fail /\ ~extracted
           /\ held'=TRUE /\ extracted'=TRUE
           /\ UNCHANGED <<caller,completed,running,alive,used>>
DropHeld == /\ held /\ held'=FALSE
            /\ alive'=(running \/ (caller /\ completed /\ ~Fail))
            /\ UNCHANGED <<caller,completed,running,extracted,used>>
Use == /\ held /\ ~used /\ used'=TRUE
       /\ UNCHANGED <<caller,completed,running,alive,held,extracted>>
Next == DropCaller \/ Complete \/ Extract \/ DropHeld \/ Use
Spec == Init /\ [][Next]_vars
TypeOK == \A x \in {caller,completed,running,alive,held,extracted,used}:x\in BOOLEAN
DynamicPolicy == (~Allow /\ ~completed) => running
CapabilityOwnership == alive=(running \/ held \/ (caller /\ completed /\ ~Fail))
UseAuthority == held => alive
=============================================================================
