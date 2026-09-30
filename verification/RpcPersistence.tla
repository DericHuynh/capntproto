------------------------- MODULE RpcPersistence -------------------------
EXTENDS Naturals, TLC
CONSTANT Fault
VARIABLES open, revoked, epoch, phase, captured, generation, capturedGeneration,
          closed, reopened, intruderTried, intruder, renewed, renewal, validDelivery,
          validRenewal
vars == <<open,revoked,epoch,phase,captured,generation,capturedGeneration,
          closed,reopened,intruderTried,intruder,renewed,renewal,validDelivery,validRenewal>>
\* Init is a committed standard Persistent.save for owner 1. Epochs 1 and 3
\* use the same key; epoch 2 uses a different key, exercising key-rotation ABA.
Key(e) == IF e=2 THEN 2 ELSE 1
Current == open /\ ~revoked /\ epoch=captured /\ generation=capturedGeneration
Init == /\ open=TRUE /\ revoked=FALSE /\ epoch=1 /\ phase=0 /\ captured=0
        /\ generation=0 /\ capturedGeneration=0 /\ closed=FALSE /\ reopened=FALSE
        /\ intruderTried=FALSE /\ intruder=FALSE /\ renewed=FALSE /\ renewal=FALSE
        /\ validDelivery=TRUE /\ validRenewal=TRUE
Begin == /\ open /\ ~revoked /\ phase=0 /\ phase'=1
         /\ captured'=epoch /\ capturedGeneration'=generation
         /\ UNCHANGED <<open,revoked,epoch,generation,closed,reopened,intruderTried,intruder,
                        renewed,renewal,validDelivery,validRenewal>>
Finish == /\ phase=1
          /\ LET accepted == (open \/ Fault="skipClose")
                          /\ (~revoked \/ Fault="skipRevoke")
                          /\ (epoch=captured \/ (Fault="skipEpoch" /\ Key(epoch)=Key(captured)))
                          /\ (generation=capturedGeneration \/ Fault="skipClose")
             IN /\ phase'=(IF accepted THEN 2 ELSE 3)
                /\ validDelivery'=(~accepted \/ Current)
          /\ UNCHANGED <<open,revoked,epoch,captured,generation,capturedGeneration,closed,reopened,
                         intruderTried,intruder,renewed,renewal,validRenewal>>
Cancel == /\ phase=1 /\ phase'=4
          /\ UNCHANGED <<open,revoked,epoch,captured,generation,capturedGeneration,closed,reopened,
                         intruderTried,intruder,renewed,renewal,validDelivery,validRenewal>>
Revoke == /\ open /\ ~revoked /\ revoked'=TRUE
          /\ UNCHANGED <<open,epoch,phase,captured,generation,capturedGeneration,closed,reopened,
                         intruderTried,intruder,renewed,renewal,validDelivery,validRenewal>>
Rotate == /\ open /\ epoch<3 /\ epoch'=epoch+1
          /\ UNCHANGED <<open,revoked,phase,captured,generation,capturedGeneration,closed,reopened,
                         intruderTried,intruder,renewed,renewal,validDelivery,validRenewal>>
Close == /\ open /\ ~closed /\ open'=FALSE /\ closed'=TRUE
         /\ UNCHANGED <<revoked,epoch,phase,captured,generation,capturedGeneration,reopened,
                        intruderTried,intruder,renewed,renewal,validDelivery,validRenewal>>
Reopen == /\ ~open /\ ~reopened /\ open'=TRUE /\ reopened'=TRUE /\ generation'=1
          /\ UNCHANGED <<revoked,epoch,phase,captured,capturedGeneration,closed,
                         intruderTried,intruder,renewed,renewal,validDelivery,validRenewal>>
Intruder == /\ open /\ ~intruderTried /\ intruderTried'=TRUE
            /\ intruder'=(Fault="skipSeal" /\ ~revoked)
            /\ UNCHANGED <<open,revoked,epoch,phase,captured,generation,capturedGeneration,closed,reopened,
                           renewed,renewal,validDelivery,validRenewal>>
Renew == /\ phase=2 /\ ~renewed /\ renewed'=TRUE
         /\ renewal'=(Current \/ Fault="skipRenewGuard")
         /\ validRenewal'=(~renewal' \/ Current)
         /\ UNCHANGED <<open,revoked,epoch,phase,captured,generation,capturedGeneration,closed,reopened,
                        intruderTried,intruder,validDelivery>>
Next == Begin \/ Finish \/ Cancel \/ Revoke \/ Rotate \/ Close \/ Reopen \/ Intruder \/ Renew
Spec == Init /\ [][Next]_vars
TypeOK == /\ epoch\in 1..3 /\ captured\in 0..3 /\ generation\in 0..1
          /\ capturedGeneration\in 0..1 /\ phase\in 0..4
          /\ \A b\in {open,revoked,closed,reopened,intruderTried,intruder,renewed,renewal,validDelivery,validRenewal}: b\in BOOLEAN
OwnerSealed == ~intruder
AuthorizedDelivery == validDelivery
AuthorizedRenewal == validRenewal
=============================================================================
