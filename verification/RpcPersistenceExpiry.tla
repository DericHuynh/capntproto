-------------------- MODULE RpcPersistenceExpiry --------------------
EXTENDS Naturals, TLC
CONSTANT Fault
\* One expiring reference, one renewal, and one fresh reference after owner
\* recreation. Clock commits and owner deletion are atomic Store snapshots.
\* Each edge prefix is replayed through real disk commits, protected save,
\* gated factory completion/cancellation, and recovery in Rust.
VARIABLES clock, deadline, owner, present, phase, captured, session, deleted, renewed, child, childDeadline, fresh, lastSerial, childSerial, validDelivery, validRenewal, clockFloor, revoked, collected, compacted
vars == <<clock,deadline,owner,present,phase,captured,session,deleted,renewed,child,childDeadline,fresh,lastSerial,childSerial,validDelivery,validRenewal,clockFloor,revoked,collected,compacted>>
Authorized == owner=1 /\ present /\ ~revoked /\ clock<deadline
Current == Authorized /\ session=captured
Allowed == (Authorized \/ Fault="skipCheck") /\ (session=captured \/ Fault="skipSession")
Init == clock=0
    /\ deadline=2
    /\ owner=1
    /\ present=TRUE
    /\ phase=0
    /\ captured=0
    /\ session=0
    /\ deleted=FALSE
    /\ renewed=FALSE
    /\ child=0
    /\ childDeadline=0
    /\ fresh=0
    /\ lastSerial=1
    /\ childSerial=0
    /\ validDelivery=TRUE
    /\ validRenewal=TRUE
    /\ clockFloor=0
    /\ revoked=FALSE /\ collected=FALSE /\ compacted=FALSE

Begin == /\ phase=0 /\ Authorized
    /\ phase'=(1)
    /\ captured'=(session)
    /\ UNCHANGED <<clock,deadline,owner,present,session,deleted,renewed,child,childDeadline,fresh,lastSerial,childSerial,validDelivery,validRenewal,clockFloor,revoked,collected,compacted>>

Finish == /\ phase=1
    /\ phase'=(IF Allowed THEN 2 ELSE 3)
    /\ validDelivery'=(~Allowed \/ Current)
    /\ UNCHANGED <<clock,deadline,owner,present,captured,session,deleted,renewed,child,childDeadline,fresh,lastSerial,childSerial,validRenewal,clockFloor,revoked,collected,compacted>>

Cancel == /\ phase=1
    /\ phase'=(4)
    /\ UNCHANGED <<clock,deadline,owner,present,captured,session,deleted,renewed,child,childDeadline,fresh,lastSerial,childSerial,validDelivery,validRenewal,clockFloor,revoked,collected,compacted>>

Shorten == /\ present /\ ~revoked /\ deadline=2
    /\ deadline'=(1)
    /\ present'=(clock<1)
    /\ UNCHANGED <<clock,owner,phase,captured,session,deleted,renewed,child,childDeadline,fresh,lastSerial,childSerial,validDelivery,validRenewal,clockFloor,revoked,collected,compacted>>

Tick == /\ clock<2
    /\ clock'=(clock+1)
    /\ clockFloor'=(clock+1)
    /\ present'=(present /\ ~revoked /\ (clock+1<deadline \/ Fault="retainExpired"))
    /\ child'=(IF child=1 /\ clock+1>=childDeadline /\ Fault#"retainChild" THEN 2 ELSE child)
    /\ UNCHANGED <<deadline,owner,phase,captured,session,deleted,renewed,childDeadline,fresh,lastSerial,childSerial,validDelivery,validRenewal,revoked,collected,compacted>>

Delete == /\ owner=1 /\ ~deleted
    /\ owner'=(0)
    /\ deleted'=(TRUE)
    /\ present'=(IF Fault="keepDeleted" THEN present ELSE FALSE)
    /\ child'=(IF child=1 THEN 2 ELSE child)
    /\ UNCHANGED <<clock,deadline,phase,captured,session,renewed,childDeadline,fresh,lastSerial,childSerial,validDelivery,validRenewal,clockFloor,revoked,collected,compacted>>

Recreate == /\ owner=0
    /\ owner'=(IF Fault="reuseEpoch" THEN 1 ELSE 2)
    /\ lastSerial'=(IF Fault="resetSerial" THEN 0 ELSE lastSerial)
    /\ UNCHANGED <<clock,deadline,present,phase,captured,session,deleted,renewed,child,childDeadline,fresh,childSerial,validDelivery,validRenewal,clockFloor,revoked,collected,compacted>>

Restart == /\ session=0
    /\ session'=(1)
    /\ clock'=(IF Fault="loseClock" THEN 0 ELSE clock)
    /\ UNCHANGED <<deadline,owner,present,phase,captured,deleted,renewed,child,childDeadline,fresh,lastSerial,childSerial,validDelivery,validRenewal,clockFloor,revoked,collected,compacted>>

Renew == /\ phase=2 /\ ~renewed
    /\ renewed'=(TRUE)
    /\ child'=(IF Current THEN 1 ELSE child)
    /\ childDeadline'=(IF Current THEN IF Fault="extendLease" THEN 3 ELSE deadline ELSE childDeadline)
    /\ childSerial'=(IF Current THEN lastSerial+1 ELSE childSerial)
    /\ lastSerial'=(IF Current THEN lastSerial+1 ELSE lastSerial)
    /\ validRenewal'=(~Current \/ Fault#"extendLease")
    /\ UNCHANGED <<clock,deadline,owner,present,phase,captured,session,deleted,fresh,validDelivery,clockFloor,revoked,collected,compacted>>

Fresh == /\ owner=2 /\ fresh=0
    /\ fresh'=(IF Fault="reuseToken" THEN 1 ELSE lastSerial+1)
    /\ lastSerial'=(IF Fault="reuseToken" THEN 1 ELSE lastSerial+1)
    /\ UNCHANGED <<clock,deadline,owner,present,phase,captured,session,deleted,renewed,child,childDeadline,childSerial,validDelivery,validRenewal,clockFloor,revoked,collected,compacted>>

Revoke == /\ present /\ ~revoked
    /\ revoked'=TRUE
    /\ UNCHANGED <<clock,deadline,owner,present,phase,captured,session,deleted,renewed,child,childDeadline,fresh,lastSerial,childSerial,validDelivery,validRenewal,clockFloor,collected,compacted>>

Collect == /\ present /\ revoked /\ ~collected
    /\ collected'=TRUE
    /\ present'=(Fault="retainRevoked")
    /\ UNCHANGED <<clock,deadline,owner,phase,captured,session,deleted,renewed,child,childDeadline,fresh,lastSerial,childSerial,validDelivery,validRenewal,clockFloor,revoked,compacted>>

Compact == /\ ~compacted
    /\ compacted'=TRUE
    /\ lastSerial'=(IF Fault="compactCounters" THEN 0 ELSE lastSerial)
    /\ clock'=(IF Fault="compactClock" THEN 0 ELSE clock)
    /\ owner'=(IF Fault="compactOwner" /\ deleted THEN 1 ELSE owner)
    /\ present'=(IF Fault="compactReference" /\ (deleted \/ clock>=deadline) THEN TRUE ELSE present)
    /\ UNCHANGED <<deadline,phase,captured,session,deleted,renewed,child,childDeadline,fresh,childSerial,validDelivery,validRenewal,clockFloor,revoked,collected>>

Next == Compact \/ Revoke \/ Collect \/ Begin \/ Finish \/ Cancel \/ Shorten \/ Tick \/ Delete \/ Recreate \/ Restart \/ Renew \/ Fresh
Spec == Init /\ [][Next]_vars
TypeOK == /\ clock\in 0..2 /\ deadline\in 1..2 /\ owner\in 0..2
          /\ phase\in 0..4 /\ captured\in 0..1 /\ session\in 0..1
          /\ child\in 0..2 /\ childDeadline\in 0..3 /\ fresh\in 0..3
          /\ lastSerial\in 0..3 /\ childSerial\in 0..2 /\ clockFloor\in 0..2
          /\ \A b\in {present,deleted,renewed,validDelivery,validRenewal,revoked,collected,compacted}: b\in BOOLEAN
AuthorizedDelivery == validDelivery
RenewalDeadline == validRenewal
ReferenceRetirement == (clock>=deadline \/ deleted) => ~present
RevokedCollection == collected => ~present
ChildRetirement == child=1 => clock<childDeadline
OwnerGeneration == deleted /\ owner#0 => owner=2
DurableTime == clock=clockFloor
UniqueIssuance == /\ lastSerial>=1
                  /\ (fresh#0 => fresh>1 /\ fresh#childSerial)
                  /\ (childSerial#0 => childSerial>1)
=============================================================================
