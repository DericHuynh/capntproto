--------------------------- MODULE RpcIncomingHints ---------------------------
EXTENDS Naturals, TLC
CONSTANTS Hold, Fail, SendReturn, DropRetained
VARIABLES completed, finished, released, used, reused, returns, alive, child
vars == <<completed,finished,released,used,reused,returns,alive,child>>
Init == /\ completed = FALSE /\ finished = FALSE /\ released = FALSE
        /\ used = FALSE /\ reused = FALSE /\ returns = 0 /\ alive = TRUE /\ child = 0
Live(c,f,r) == IF f THEN Hold /\ ~r ELSE ~(Fail /\ c /\ (~Hold \/ r))
Complete == /\ ~completed /\ ~finished /\ completed' = TRUE
            /\ alive' = Live(TRUE,finished,released)
            /\ returns' = IF SendReturn THEN 1 ELSE returns
            /\ UNCHANGED <<finished,released,used,reused,child>>
Finish == /\ ~finished /\ finished' = TRUE
          /\ alive' = IF DropRetained THEN FALSE ELSE Live(completed,TRUE,released)
          /\ UNCHANGED <<completed,released,used,reused,returns,child>>
Release == /\ Hold /\ ~released /\ released' = TRUE
           /\ alive' = Live(completed,finished,TRUE)
           /\ UNCHANGED <<completed,finished,used,reused,returns,child>>
Use == /\ completed /\ (~Hold \/ released) /\ ~finished /\ ~used /\ used' = TRUE
       /\ child' = IF Fail THEN 2 ELSE 1
       /\ UNCHANGED <<completed,finished,released,reused,returns,alive>>
Reuse == /\ finished /\ (~Hold \/ released) /\ ~reused /\ reused' = TRUE
         /\ UNCHANGED <<completed,finished,released,used,returns,alive,child>>
Next == Complete \/ Finish \/ Release \/ Use \/ Reuse
Spec == Init /\ [][Next]_vars
TypeOK == /\ completed \in BOOLEAN /\ finished \in BOOLEAN /\ released \in BOOLEAN
          /\ used \in BOOLEAN /\ reused \in BOOLEAN /\ returns \in 0..1 /\ alive \in BOOLEAN /\ child \in 0..2
NoReturn == returns = 0
ContextLifetime == alive = Live(completed,finished,released)
ChildProvenance == used => child = (IF Fail THEN 2 ELSE 1)
=============================================================================
