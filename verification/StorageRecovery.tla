----------------------- MODULE StorageRecovery -----------------------
EXTENDS Naturals
CONSTANT Fault
VARIABLES visible, durable, ack, phase, epoch, durableEpoch, damaged,
          rejected, rewritten, failures, crashes, powerLosses, torn,
          partials, recovered
vars == <<visible,durable,ack,phase,epoch,durableEpoch,damaged,rejected,
          rewritten,failures,crashes,powerLosses,torn,partials,recovered>>
Init == /\ visible=0 /\ durable=0 /\ ack=0 /\ phase=0
        /\ epoch=0 /\ durableEpoch=0 /\ damaged=0 /\ rejected=0
        /\ rewritten=0 /\ failures=0 /\ crashes=0 /\ powerLosses=0
        /\ torn=0 /\ partials=0 /\ recovered=0
Commit == /\ phase=0 /\ visible<2
          /\ visible'=visible+1 /\ durable'=visible+1 /\ ack'=visible+1
          /\ recovered'=0
          /\ UNCHANGED <<phase,epoch,durableEpoch,damaged,rejected,rewritten,
                         failures,crashes,powerLosses,torn,partials>>
Uncertain(synced) ==
    /\ phase=0 /\ visible<2
    /\ visible'=visible+1 /\ durable'=IF synced THEN visible+1 ELSE durable
    /\ phase'=1 /\ recovered'=0
    /\ UNCHANGED <<ack,epoch,durableEpoch,damaged,rejected,rewritten,failures,
                   crashes,powerLosses,torn,partials>>
Partial == /\ phase=0 /\ partials=0
           /\ phase'=1 /\ partials'=1 /\ torn'=1 /\ recovered'=0
           /\ UNCHANGED <<visible,durable,ack,epoch,durableEpoch,damaged,
                          rejected,rewritten,failures,crashes,powerLosses>>
Rename == /\ phase=0 /\ epoch=0 /\ visible>0
          /\ epoch'=1 /\ phase'=1 /\ recovered'=0
          /\ UNCHANGED <<visible,durable,ack,durableEpoch,damaged,rejected,
                         rewritten,failures,crashes,powerLosses,torn,partials>>
Crash == /\ phase # 2 /\ crashes<2
         /\ phase'=2 /\ crashes'=crashes+1 /\ recovered'=0
         /\ UNCHANGED <<visible,durable,ack,epoch,durableEpoch,damaged,
                        rejected,rewritten,failures,powerLosses,torn,partials>>
PowerLoss == /\ phase=2 /\ damaged=0 /\ powerLosses=0
             /\ visible'=durable /\ epoch'=durableEpoch /\ powerLosses'=1
             /\ torn'=0 /\ recovered'=0
             /\ UNCHANGED <<durable,ack,phase,durableEpoch,damaged,rejected,
                            rewritten,failures,crashes,partials>>
Recover == /\ phase=2 /\ damaged=0
           /\ phase'=0 /\ torn'=0 /\ recovered'=1
           /\ durable'=IF Fault="skipFileSync" THEN durable ELSE visible
           /\ durableEpoch'=IF Fault="skipDirectorySync" THEN durableEpoch ELSE epoch
           /\ UNCHANGED <<visible,ack,epoch,damaged,rejected,rewritten,failures,
                          crashes,powerLosses,partials>>
FailRecover == /\ phase=2 /\ damaged=0 /\ failures=0
               /\ failures'=1 /\ torn'=0
               /\ UNCHANGED <<visible,durable,ack,phase,epoch,durableEpoch,damaged,
                              rejected,rewritten,crashes,powerLosses,partials,recovered>>
CorruptLength == /\ phase=2 /\ visible>0 /\ epoch=0 /\ torn=0 /\ damaged=0
                 /\ damaged'=1
                 /\ UNCHANGED <<visible,durable,ack,phase,epoch,durableEpoch,
                                rejected,rewritten,failures,crashes,powerLosses,torn,partials,recovered>>
RejectCorrupt == /\ phase=2 /\ damaged=1 /\ rejected=0
                 /\ rejected'=1 /\ rewritten'=IF Fault="trustLength" THEN 1 ELSE 0
                 /\ UNCHANGED <<visible,durable,ack,phase,epoch,durableEpoch,
                                damaged,failures,crashes,powerLosses,torn,partials,recovered>>
Next == Commit \/ Uncertain(FALSE) \/ Uncertain(TRUE) \/ Partial \/ Rename
        \/ Crash \/ PowerLoss \/ Recover \/ FailRecover \/ CorruptLength \/ RejectCorrupt
Spec == Init /\ [][Next]_vars
TypeOK == /\ visible \in 0..2 /\ durable \in 0..visible /\ ack \in 0..2
          /\ phase \in 0..2 /\ crashes \in 0..2
          /\ \A x \in {epoch,durableEpoch,damaged,rejected,rewritten,failures,
                        powerLosses,torn,partials,recovered}: x \in 0..1
AcknowledgedDurable == ack <= durable
RecoveryStabilized == recovered=1 => visible=durable /\ epoch=durableEpoch /\ torn=0
CorruptionNeverRewrites == rewritten=0
ServingHealthy == phase=0 => damaged=0 /\ torn=0
=============================================================================
