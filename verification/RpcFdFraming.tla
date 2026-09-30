---------------------------- MODULE RpcFdFraming ----------------------------
EXTENDS Naturals, TLC
CONSTANTS KeepFd, EarlyDelivery, DuplicateAttachment
VARIABLES written, started, prefix, done, closed, attachments, received
vars == <<written,started,prefix,done,closed,attachments,received>>
Init == /\ written = 0 /\ started = FALSE /\ prefix = FALSE
        /\ done = 0 /\ closed = FALSE /\ attachments = 0 /\ received = 0
First == /\ written = 0 /\ ~closed /\ written' = 1 /\ attachments' = 1
         /\ UNCHANGED <<started,prefix,done,closed,received>>
Rest == /\ written = 1 /\ ~closed /\ written' = 2
        /\ attachments' = IF DuplicateAttachment THEN 2 ELSE attachments
        /\ UNCHANGED <<started,prefix,done,closed,received>>
Start == /\ ~started /\ ~closed /\ started' = TRUE
         /\ UNCHANGED <<written,prefix,done,closed,attachments,received>>
\* Consume the first segment-header word and ancillary data, but not the remainder.
Prefix == /\ started /\ ~prefix /\ done = 0 /\ written = 1 /\ prefix' = TRUE
          /\ UNCHANGED <<written,started,done,closed,attachments,received>>
Complete == /\ started /\ done = 0
            /\ (written = 2 \/ (EarlyDelivery /\ written = 1))
            /\ done' = 1 /\ received' = IF KeepFd THEN 1 ELSE 0
            /\ UNCHANGED <<written,started,prefix,closed,attachments>>
Cancel == /\ started /\ done = 0 /\ done' = 2 /\ closed' = TRUE
          /\ UNCHANGED <<written,started,prefix,attachments,received>>
Eof == /\ started /\ done = 0 /\ written < 2
       /\ done' = (IF written = 0 THEN 3 ELSE 4) /\ closed' = TRUE
       /\ UNCHANGED <<written,started,prefix,attachments,received>>
Next == First \/ Rest \/ Start \/ Prefix \/ Complete \/ Cancel \/ Eof
Spec == Init /\ [][Next]_vars
TypeOK == /\ written \in 0..2 /\ started \in BOOLEAN /\ prefix \in BOOLEAN
          /\ done \in 0..4 /\ closed \in BOOLEAN /\ attachments \in 0..2 /\ received \in 0..1
WholeFrame == done = 1 => written = 2
SingleAttachment == attachments <= 1
DescriptorDelivery == received = 1 => done = 1 /\ KeepFd
CancellationBoundary == done \in {2,3,4} => closed /\ received = 0
=============================================================================
