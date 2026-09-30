-------------------------- MODULE RpcFileDescriptors --------------------------
EXTENDS Naturals, TLC
CONSTANTS Limit, SharedSlot, SameImport, AllowFd, ReuseSlot, Overwrite
VARIABLES delivered, reimported, observed, revoked, left, right, seenLeft, seenRight
vars == <<delivered,reimported,observed,revoked,left,right,seenLeft,seenRight>>
First == IF Limit > 0 THEN 1 ELSE 0
Second == IF SharedSlot THEN 0 ELSE IF Limit > 1 THEN 2 ELSE 0
LeftExpected == IF First = 0 /\ reimported THEN 3 ELSE First
RightExpected == IF SameImport THEN LeftExpected ELSE Second
Init == /\ delivered = FALSE /\ reimported = FALSE /\ observed = FALSE
        /\ revoked = FALSE /\ left = 0 /\ right = 0 /\ seenLeft = 0 /\ seenRight = 0
\* One Call imports two descriptors. Each ancillary slot is consumed once,
\* even when a different capability maliciously names the same slot. Duplicate
\* export IDs alias an existing import, which retains its first descriptor.
Deliver == /\ ~delivered /\ delivered' = TRUE /\ left' = First
           /\ right' = IF SameImport THEN First
                       ELSE IF ReuseSlot /\ SharedSlot THEN First ELSE Second
           /\ UNCHANGED <<reimported,observed,revoked,seenLeft,seenRight>>
Reimport == /\ delivered /\ ~reimported /\ reimported' = TRUE
            /\ left' = IF left = 0 \/ Overwrite THEN 3 ELSE left
            /\ right' = IF SameImport THEN left' ELSE right
            /\ UNCHANGED <<delivered,observed,revoked,seenLeft,seenRight>>
\* This is the synchronous hook; the public async accessor follows resolution
\* only when the hook has no descriptor (separately exercised by Rust tests).
Observe == /\ delivered /\ ~observed /\ observed' = TRUE
           /\ seenLeft' = IF AllowFd /\ ~revoked THEN left ELSE 0
           /\ seenRight' = IF AllowFd /\ ~revoked THEN right ELSE 0
           /\ UNCHANGED <<delivered,reimported,revoked,left,right>>
\* Descriptors already delivered escape revocation, just like any OS authority.
Revoke == /\ delivered /\ ~revoked /\ revoked' = TRUE
          /\ UNCHANGED <<delivered,reimported,observed,left,right,seenLeft,seenRight>>
Next == Deliver \/ Reimport \/ Observe \/ Revoke
Spec == Init /\ [][Next]_vars
TypeOK == /\ delivered \in BOOLEAN /\ reimported \in BOOLEAN
          /\ observed \in BOOLEAN /\ revoked \in BOOLEAN
          /\ left \in 0..3 /\ right \in 0..3 /\ seenLeft \in 0..3 /\ seenRight \in 0..3
FirstDescriptorWins == delivered => left = LeftExpected
SingleOwner == delivered => right = RightExpected
MembraneAuthority == ~AllowFd => seenLeft = 0 /\ seenRight = 0
=============================================================================
