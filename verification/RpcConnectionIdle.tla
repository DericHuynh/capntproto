------------------------ MODULE RpcConnectionIdle ------------------------
EXTENDS Naturals, TLC
CONSTANTS EndMode, Retained, IgnoreCaps, IgnoreCall, AbortIdle, SkipWake
VARIABLES cap,q,connected,probed,idle,closed,aborts,events,clean,ever,held,finished,call
vars == <<cap,q,connected,probed,idle,closed,aborts,events,clean,ever,held,finished,call>>
N(x) == IF x THEN 1 ELSE 0
Empty == ~cap /\ ~q /\ ~call
Init == /\ cap=TRUE /\ q=TRUE /\ connected=TRUE /\ probed=FALSE /\ idle=FALSE
        /\ closed=FALSE /\ aborts=0 /\ events=1 /\ clean=FALSE /\ ever=FALSE
        /\ held=Retained /\ finished=FALSE /\ call=Retained
Cap == /\ cap /\ cap'=FALSE /\ UNCHANGED <<q,connected,probed,closed,aborts,clean,held,finished>>
Question == /\ q /\ q'=FALSE /\ UNCHANGED <<cap,connected,probed,closed,aborts,clean,held,finished>>
Held == /\ held /\ held'=FALSE /\ UNCHANGED <<cap,q,connected,probed,closed,aborts,clean,finished>>
Finish == /\ Retained /\ ~finished /\ finished'=TRUE
          /\ UNCHANGED <<cap,q,connected,probed,closed,aborts,clean,held>>
Probe == /\ connected /\ Empty /\ ~probed /\ probed'=TRUE
         /\ UNCHANGED <<cap,q,connected,closed,aborts,clean,held,finished>>
EOF == /\ connected /\ connected'=FALSE /\ closed'=TRUE /\ clean'=(idle /\ EndMode=0)
       /\ aborts'=(IF idle /\ ~AbortIdle THEN 0 ELSE 1)
       /\ UNCHANGED <<cap,q,probed,held,finished>>
Next == /\ (Cap \/ Question \/ Held \/ Finish \/ Probe \/ EOF)
        /\ call'=(Retained /\ (~finished' \/ held'))
        /\ idle'=(IF connected' THEN (~q' /\ (IgnoreCaps \/ ~cap') /\ (IgnoreCall \/ ~call')) ELSE idle)
        /\ ever'=(ever \/ (connected' /\ Empty'))
        /\ events'=events+N(idle' /\ ~idle)+(IF probed' /\ ~probed THEN (IF SkipWake THEN 1 ELSE 2) ELSE 0)
Spec == Init /\ [][Next]_vars
TypeOK == /\ \A x\in {cap,q,connected,probed,idle,closed,clean,ever,held,finished,call}: x\in BOOLEAN
          /\ aborts\in 0..1 /\ events\in 1..4
AllTables == connected => (idle=Empty)
CleanEOF == closed => (aborts=N(~idle))
ShutdownKind == closed => (clean=(idle /\ EndMode=0))
Notifications == events=1+N(ever)+2*N(probed)
=============================================================================
