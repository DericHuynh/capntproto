------------------------- MODULE RpcFieldStaging -------------------------
EXTENDS Naturals, TLC
CONSTANTS PublishDraft, DamageOnError, PrematureReady
VARIABLES phase, filled, slot, event, readyAtCommit
vars == <<phase,filled,slot,event,readyAtCommit>>
Init == /\ phase=0 /\ filled=0 /\ slot=1 /\ event=0 /\ readyAtCommit=FALSE
Stage == /\ phase=0 /\ phase'=1 /\ event'=1 /\ UNCHANGED <<filled,slot,readyAtCommit>>
Chunk == /\ phase=1 /\ filled<2 /\ filled'=filled+1 /\ event'=2 /\ UNCHANGED <<phase,slot,readyAtCommit>>
Ready == /\ phase=1 /\ (filled=2 \/ PrematureReady) /\ phase'=2 /\ event'=3
         /\ UNCHANGED <<filled,slot,readyAtCommit>>
Fail == /\ phase=1 /\ phase'=3 /\ event'=4 /\ slot'=IF DamageOnError THEN 0 ELSE slot
        /\ UNCHANGED <<filled,readyAtCommit>>
Drop == /\ phase\in {1,2} /\ phase'=3 /\ event'=5 /\ UNCHANGED <<filled,slot,readyAtCommit>>
Forget == /\ phase\in {1,2} /\ phase'=3 /\ event'=6 /\ UNCHANGED <<filled,slot,readyAtCommit>>
Commit == /\ (phase=2 \/ (PublishDraft /\ phase=1)) /\ phase'=4 /\ slot'=2 /\ event'=7
          /\ readyAtCommit'=(phase=2) /\ UNCHANGED filled
Next == Stage \/ Chunk \/ Ready \/ Fail \/ Drop \/ Forget \/ Commit
Spec == Init /\ [][Next]_vars
TypeOK == /\ phase\in 0..4 /\ filled\in 0..2 /\ slot\in 0..2 /\ event\in 0..7 /\ readyAtCommit\in BOOLEAN
NoEarlyPublication == phase#4 => slot=1
ReadyIsComplete == phase=2 => filled=2
Publication == phase=4 => (readyAtCommit /\ filled=2 /\ slot=2)
=============================================================================
