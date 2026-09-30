--------------------------- MODULE RpcImportAlias ---------------------------
EXTENDS Naturals, TLC
CONSTANT LoseRegistration
VARIABLES acquired, dropped, firstUse, lastUse, registered
vars == <<acquired,dropped,firstUse,lastUse,registered>>
\* Original import remains alive. A second descriptor names the same export.
Init == /\ acquired = FALSE /\ dropped = FALSE /\ firstUse = FALSE
        /\ lastUse = FALSE /\ registered = TRUE
Acquire == /\ ~acquired /\ acquired' = TRUE
           /\ UNCHANGED <<dropped,firstUse,lastUse,registered>>
Drop == /\ acquired /\ ~dropped /\ dropped' = TRUE /\ registered' = ~LoseRegistration
        /\ UNCHANGED <<acquired,firstUse,lastUse>>
UseFirst == /\ ~firstUse /\ registered /\ firstUse' = TRUE
            /\ UNCHANGED <<acquired,dropped,lastUse,registered>>
UseLast == /\ dropped /\ ~lastUse /\ registered /\ lastUse' = TRUE
           /\ UNCHANGED <<acquired,dropped,firstUse,registered>>
Next == Acquire \/ Drop \/ UseFirst \/ UseLast
Spec == Init /\ [][Next]_vars
TypeOK == /\ acquired \in BOOLEAN /\ dropped \in BOOLEAN /\ firstUse \in BOOLEAN
          /\ lastUse \in BOOLEAN /\ registered \in BOOLEAN
LiveImportRegistered == registered
AliasProvenance == dropped => acquired
=============================================================================
