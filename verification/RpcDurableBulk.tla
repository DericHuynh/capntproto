-------------------- MODULE RpcDurableBulk --------------------
EXTENDS Naturals, TLC
CONSTANTS Extra, ValidDigest, Fault
\* Two immutable chunks. Completion atomically commits the target and receipt.
\* Extra: 0=none, 1=restart, 2=history checkpoint, 3=revoke, 4=target edit,
\* 5=conflicting retry. RPC actions are observed after executor drain; separately
\* tested lost replies and byte-truncated batches refine the commit boundary.
VARIABLES chunks, acknowledged, phase, receipt, published, visible, journal,
          bulkCommit, live, extra, retried, doneCalls, canceled, denied, blocked,
          frozenChunks, frozenReceipt, closedChunks, validPrefix, event, result
vars == <<chunks,acknowledged,phase,receipt,published,visible,journal,bulkCommit,
          live,extra,retried,doneCalls,canceled,denied,blocked,frozenChunks,
          frozenReceipt,closedChunks,validPrefix,event,result>>
\* phase: receiving=0, complete=1, canceled=2, failed=3.
\* result: ordinary success=1, error=2, complete=3, canceled=4, failed=5.
Init == /\ chunks=0 /\ acknowledged=0 /\ phase=0 /\ receipt=0 /\ published=0
        /\ visible=0 /\ journal=1 /\ bulkCommit=FALSE /\ live=TRUE
        /\ extra=FALSE /\ retried=FALSE /\ doneCalls=0 /\ canceled=FALSE
        /\ denied=FALSE /\ blocked=FALSE /\ frozenChunks=0 /\ frozenReceipt=0
        /\ closedChunks=0 /\ validPrefix=TRUE /\ event=0 /\ result=0
Write == /\ chunks<2 /\ (live \/ Fault="ignoreRevoke")
         /\ (phase=0 \/ (Fault="cancelLeak" /\ phase=2))
         /\ chunks'=chunks+1 /\ acknowledged'=acknowledged+1 /\ journal'=journal+1
         /\ event'=1 /\ result'=1
         /\ UNCHANGED <<phase,receipt,published,visible,bulkCommit,live,extra,retried,
              doneCalls,canceled,denied,blocked,frozenChunks,frozenReceipt,closedChunks,validPrefix>>
Retry == /\ chunks>0 /\ ~retried /\ retried'=TRUE /\ event'=2
         /\ result'=(IF live /\ phase\in {0,1} THEN 1 ELSE 2)
         /\ chunks'=(IF Fault="duplicate" /\ live /\ phase=0 THEN chunks+1 ELSE chunks)
         /\ UNCHANGED <<acknowledged,phase,receipt,published,visible,journal,bulkCommit,live,
              extra,doneCalls,canceled,denied,blocked,frozenChunks,frozenReceipt,closedChunks,validPrefix>>
Done == /\ doneCalls<2 /\ doneCalls'=doneCalls+1 /\ event'=3
        /\ IF live /\ phase=0 /\ (chunks=2 \/ Fault="earlyPublish")
               /\ (~blocked \/ Fault="overwrite") /\ ValidDigest
           THEN /\ phase'=(IF Fault="noReceipt" THEN 0 ELSE 1)
                /\ receipt'=(IF Fault="noReceipt" THEN 0 ELSE published+1)
                /\ published'=published+1 /\ visible'=1 /\ bulkCommit'=TRUE
                /\ journal'=(IF Fault="noReceipt" THEN journal ELSE journal+1)
                /\ result'=3
           ELSE IF live /\ phase=0 /\ chunks=2 /\ ~ValidDigest
                THEN /\ phase'=3 /\ journal'=journal+1 /\ result'=2
                     /\ UNCHANGED <<receipt,published,visible,bulkCommit>>
                ELSE /\ result'=(IF live /\ phase=1 THEN 3 ELSE 2)
                     /\ UNCHANGED <<phase,receipt,published,visible,bulkCommit,journal>>
        /\ UNCHANGED <<chunks,acknowledged,live,extra,retried,canceled,denied,blocked,
             frozenChunks,frozenReceipt,closedChunks,validPrefix>>
Cancel == /\ ~canceled /\ canceled'=TRUE /\ event'=4
          /\ phase'=(IF live /\ phase=0 THEN 2 ELSE phase)
          /\ journal'=(IF live /\ phase=0 THEN journal+1 ELSE journal)
          /\ closedChunks'=chunks
          /\ result'=(IF ~live THEN 2 ELSE CASE phase=1 -> 3 [] phase=3 -> 5 [] OTHER -> 4)
          /\ UNCHANGED <<chunks,acknowledged,receipt,published,visible,bulkCommit,live,extra,
               retried,doneCalls,denied,blocked,frozenChunks,frozenReceipt,validPrefix>>
ExtraAction == /\ Extra#0 /\ ~extra /\ (Extra#5 \/ chunks>0)
               /\ extra'=TRUE /\ event'=5 /\ result'=(IF Extra=5 THEN 2 ELSE 1)
               /\ chunks'=(IF (Extra=1 /\ Fault="loseAck") \/ (Extra=2 /\ Fault="loseStaging") THEN 0 ELSE chunks)
               /\ live'=(IF Extra=3 THEN FALSE ELSE live)
               /\ frozenChunks'=chunks /\ frozenReceipt'=receipt
               /\ published'=(IF Extra=4 THEN published+1 ELSE published)
               /\ visible'=(IF Extra=4 THEN 2 ELSE visible)
               /\ blocked'=(IF Extra=4 THEN ~bulkCommit ELSE blocked)
               /\ validPrefix'=(IF Extra=5 /\ Fault="replaceRetry" THEN FALSE ELSE validPrefix)
               /\ UNCHANGED <<acknowledged,phase,receipt,journal,bulkCommit,retried,
                    doneCalls,canceled,denied,closedChunks>>
DeniedWrite == /\ ~denied /\ (~live \/ phase#0) /\ chunks<2
               /\ denied'=TRUE /\ event'=6 /\ result'=2
               /\ UNCHANGED <<chunks,acknowledged,phase,receipt,published,visible,journal,
                    bulkCommit,live,extra,retried,doneCalls,canceled,blocked,frozenChunks,
                    frozenReceipt,closedChunks,validPrefix>>
Next == Write \/ Retry \/ Done \/ Cancel \/ ExtraAction \/ DeniedWrite
Spec == Init /\ [][Next]_vars
TypeOK == /\ chunks\in 0..3 /\ acknowledged\in 0..3 /\ phase\in 0..3
          /\ receipt\in 0..3 /\ published\in 0..3 /\ visible\in 0..2 /\ journal\in 1..6
          /\ doneCalls\in 0..2 /\ event\in 0..6 /\ result\in 0..5
          /\ frozenChunks\in 0..3 /\ frozenReceipt\in 0..3 /\ closedChunks\in 0..3
          /\ \A b\in {bulkCommit,live,extra,retried,canceled,denied,blocked,validPrefix}: b\in BOOLEAN
DurablePrefix == chunks=acknowledged
ImmutablePrefix == validPrefix
CompletionSound == phase=1 => chunks=2 /\ ValidDigest /\ receipt>0
AtomicReceipt == (phase=1)=bulkCommit /\ ((receipt>0)=(phase=1)) /\ receipt<=published
NoRevokedEffects == ~live => chunks=frozenChunks /\ receipt=frozenReceipt
NoCanceledWrites == phase=2 => chunks=closedChunks
TargetCAS == blocked => ~bulkCommit
HealthySpec == Spec /\ WF_vars(Write) /\ WF_vars(Done) /\ WF_vars(Cancel)
Terminates == (phase=0) ~> (phase#0 \/ ~live \/ blocked)
=============================================================================
