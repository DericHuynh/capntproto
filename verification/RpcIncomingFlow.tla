------------------------ MODULE RpcIncomingFlow ------------------------
EXTENDS Naturals, TLC
CONSTANTS HoldContext, PreCanceled, FailFirst, WakeAtEquality, ReleaseOnCancel
VARIABLES first, second, held, finish, probe, limit, paused, r1, r2,
          connected, wakeWords, wakeLimit
vars == <<first,second,held,finish,probe,limit,paused,r1,r2,connected,wakeWords,wakeLimit>>
State == [first |-> first, second |-> second, held |-> held, finish |-> finish,
          probe |-> probe, limit |-> limit, paused |-> paused, r1 |-> r1,
          r2 |-> r2, connected |-> connected, wakeWords |-> wakeWords, wakeLimit |-> wakeLimit]
\* Each call is one unit of its actual wire message size. Initially two gated
\* calls have been dispatched: the first reaches limit=1, the second crosses it.
\* 0=running, 1=completed, 2=canceled for call tasks. Return 0=none, 1=results,
\* 2=canceled, 3=exception. Finish and a non-call Bootstrap probe: 0=unsent, 1=queued, 2=read.
\* PreCanceled starts after a canceled, retained context and a pending read
\* completed following a limit reduction. Replay constructs this prefix.
Init == /\ first = (IF PreCanceled THEN 2 ELSE 0) /\ second = 0
        /\ held = HoldContext /\ finish = (IF PreCanceled THEN 2 ELSE 0) /\ probe = 0
        /\ limit = 1 /\ paused = TRUE /\ r1 = 0 /\ r2 = 0 /\ connected = TRUE
        /\ wakeWords = 0 /\ wakeLimit = 1
ExpectedWords(s) == (IF s.r1 = 0 THEN 1 ELSE 0) + (IF s.r2 = 0 THEN 1 ELSE 0)
Words(s) == IF ReleaseOnCancel /\ s.first = 2 /\ s.r1 = 0
            THEN ExpectedWords(s) - 1 ELSE ExpectedWords(s)
Wake(s) == IF s.paused /\ (Words(s) < s.limit \/ (WakeAtEquality /\ Words(s) = s.limit))
           THEN [s EXCEPT !.paused = FALSE, !.wakeWords = ExpectedWords(s), !.wakeLimit = s.limit]
           ELSE s
\* All control messages are behind the same paused reader. Once awake, both
\* can drain; their order has no effect on this model's final state. Wire replay
\* checks Return counts/kinds, not the relative order of independent replies.
Drain(s) == LET w == Wake(s)
            IN IF w.paused THEN w ELSE
               [w EXCEPT !.finish = IF w.finish = 1 THEN 2 ELSE @,
                         !.probe = IF w.probe = 1 THEN 2 ELSE @,
                         !.first = IF w.finish = 1 /\ w.first = 0 THEN 2 ELSE @,
                         !.r1 = IF w.finish = 1 /\ w.first = 0 /\ ~w.held THEN 2 ELSE @]
Commit(s) == /\ first' = s.first
             /\ second' = s.second
             /\ held' = s.held
             /\ finish' = s.finish
             /\ probe' = s.probe
             /\ limit' = s.limit
             /\ paused' = s.paused
             /\ r1' = s.r1
             /\ r2' = s.r2
             /\ connected' = s.connected
             /\ wakeWords' = s.wakeWords
             /\ wakeLimit' = s.wakeLimit
CompleteFirst == /\ connected /\ first = 0
                 /\ Commit(Drain([State EXCEPT !.first = 1, !.held = FALSE, !.r1 = IF FailFirst THEN 3 ELSE 1]))
CompleteSecond == /\ connected /\ second = 0
                  /\ Commit(Drain([State EXCEPT !.second = 1, !.r2 = 1]))
SendFinish == /\ connected /\ finish = 0 /\ Commit(Drain([State EXCEPT !.finish = 1]))
SendProbe == /\ connected /\ probe = 0 /\ Commit(Drain([State EXCEPT !.probe = 1]))
SetLimit(n) == /\ connected /\ paused /\ n \in {0,2,3} /\ limit # n
              /\ Commit(Drain([State EXCEPT !.limit = n]))
DropContext == /\ held /\ first = 2
               /\ LET s == [State EXCEPT !.held = FALSE, !.r1 = IF connected THEN 2 ELSE @]
                  IN Commit(IF connected THEN Drain(s) ELSE s)
Disconnect == /\ connected
              /\ Commit([State EXCEPT !.connected = FALSE, !.paused = FALSE,
                          !.first = IF first = 0 THEN 2 ELSE @,
                          !.second = IF second = 0 THEN 2 ELSE @])
Next == CompleteFirst \/ CompleteSecond \/ SendFinish \/ SendProbe
        \/ (\E n \in {0,2,3}: SetLimit(n)) \/ DropContext \/ Disconnect
Spec == Init /\ [][Next]_vars
TypeOK == /\ first \in 0..2 /\ second \in 0..2 /\ held \in BOOLEAN
          /\ finish \in 0..2 /\ probe \in 0..2 /\ limit \in 0..3 /\ paused \in BOOLEAN
          /\ r1 \in 0..3 /\ r2 \in 0..1 /\ connected \in BOOLEAN
          /\ wakeWords \in 0..2 /\ wakeLimit \in 0..3
StrictWake == wakeWords < wakeLimit
CreditUntilReturn == connected => Words(State) = ExpectedWords(State)
HeldContext == held => r1 = 0
QueuedControl == (connected /\ paused) => probe # 2
ReturnFollowsWork == /\ (r1 \in {1,3} => first = 1) /\ (r1 = 2 => first = 2)
                    /\ (r2 = 1 => second = 1)
=============================================================================
