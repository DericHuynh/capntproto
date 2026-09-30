---------------------- MODULE RpcExceptionDisconnect ----------------------
EXTENDS Naturals, TLC
CONSTANTS WrongKind, LoseMetadata
VARIABLES aborted, created, first, second, third, kind, metadata
vars == <<aborted,created,first,second,third,kind,metadata>>
\* Two calls are pending; a third may be made before or after the peer's Abort.
Init == /\ aborted = FALSE /\ created = FALSE /\ first = FALSE
        /\ second = FALSE /\ third = FALSE /\ kind = 0 /\ metadata = 0
Abort == /\ ~aborted /\ aborted' = TRUE
         /\ kind' = IF WrongKind THEN 1 ELSE 2
         /\ metadata' = IF LoseMetadata THEN 0 ELSE 1
         /\ UNCHANGED <<created,first,second,third>>
Create == /\ ~created /\ created' = TRUE
          /\ UNCHANGED <<aborted,first,second,third,kind,metadata>>
First == /\ aborted /\ ~first /\ first' = TRUE
         /\ UNCHANGED <<aborted,created,second,third,kind,metadata>>
Second == /\ aborted /\ ~second /\ second' = TRUE
          /\ UNCHANGED <<aborted,created,first,third,kind,metadata>>
Third == /\ aborted /\ created /\ ~third /\ third' = TRUE
         /\ UNCHANGED <<aborted,created,first,second,kind,metadata>>
Next == Abort \/ Create \/ First \/ Second \/ Third
Spec == Init /\ [][Next]_vars
TypeOK == /\ aborted \in BOOLEAN /\ created \in BOOLEAN /\ first \in BOOLEAN
          /\ second \in BOOLEAN /\ third \in BOOLEAN /\ kind \in 0..2 /\ metadata \in 0..1
DisconnectedKind == aborted => kind = 2
PreservedDiagnostics == aborted => metadata = 1
NoEarlyFailure == (first \/ second \/ third) => aborted
=============================================================================
