----------------------- MODULE RpcExceptionMetadata -----------------------
EXTENDS Naturals, TLC
CONSTANTS Trace, LoseDetails, LeakTrace, DoublePrefix
VARIABLES received, cloned, dropped, forwarded, decoded, config, trace, details,
          prefixes, expectedTrace
vars == <<received,cloned,dropped,forwarded,decoded,config,trace,details,prefixes,expectedTrace>>
\* One exception received, copied, forwarded as another RPC failure and decoded.
\* Details abstract two owned entries; the Rust replay also checks duplicate IDs,
\* opaque bytes, empty-vs-absent data, and all wire kinds including an unknown kind.
Init == /\ received = FALSE /\ cloned = FALSE /\ dropped = FALSE
        /\ forwarded = FALSE /\ decoded = FALSE /\ config = 0
        /\ trace = 0 /\ details = 0 /\ prefixes = 0 /\ expectedTrace = 0
Receive == /\ ~received /\ received' = TRUE
           /\ trace' = (IF Trace THEN 1 ELSE 0) /\ details' = 2 /\ prefixes' = 1
           /\ UNCHANGED <<cloned,dropped,forwarded,decoded,config,expectedTrace>>
Clone == /\ received /\ ~cloned /\ cloned' = TRUE
         /\ details' = IF LoseDetails THEN 0 ELSE details
         /\ UNCHANGED <<received,dropped,forwarded,decoded,config,trace,prefixes,expectedTrace>>
Drop == /\ cloned /\ ~dropped /\ dropped' = TRUE
        /\ UNCHANGED <<received,cloned,forwarded,decoded,config,trace,details,prefixes,expectedTrace>>
Configure == /\ config < 2 /\ config' = config + 1
             /\ UNCHANGED <<received,cloned,dropped,forwarded,decoded,trace,details,prefixes,expectedTrace>>
Forward == /\ cloned /\ ~forwarded /\ forwarded' = TRUE
           /\ expectedTrace' = IF config = 1 THEN 2 ELSE 0
           /\ trace' = IF config = 1 THEN 2 ELSE IF LeakTrace THEN trace ELSE 0
           /\ UNCHANGED <<received,cloned,dropped,decoded,config,details,prefixes>>
Decode == /\ forwarded /\ ~decoded /\ decoded' = TRUE
          /\ prefixes' = IF DoublePrefix THEN prefixes + 1 ELSE prefixes
          /\ UNCHANGED <<received,cloned,dropped,forwarded,config,trace,details,expectedTrace>>
Next == Receive \/ Clone \/ Drop \/ Configure \/ Forward \/ Decode
Spec == Init /\ [][Next]_vars
TypeOK == /\ received \in BOOLEAN /\ cloned \in BOOLEAN /\ dropped \in BOOLEAN
          /\ forwarded \in BOOLEAN /\ decoded \in BOOLEAN /\ config \in 0..2
          /\ trace \in 0..2 /\ details \in 0..2 /\ prefixes \in 0..2 /\ expectedTrace \in 0..2
OwnedDetails == received => details = 2
TracePolicy == forwarded => trace = expectedTrace
SinglePrefix == received => prefixes = 1
=============================================================================
