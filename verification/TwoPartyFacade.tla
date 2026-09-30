---------------------------- MODULE TwoPartyFacade ----------------------------
EXTENDS Naturals
CONSTANT Fault
\* One owned and one borrowed connection; at most five lifecycle actions.
\* IO is quiescent after each action. A call is a real capability invocation in
\* both replayers, but wire scheduling, framing and cancellation races are not
\* abstracted here. Server means the connection owner/driver, not Rust's handle.
VARIABLES owned, borrowed, stopped, drain, ownedDrops, borrowedDrops,
          calledOwned, calledBorrowed, calls, borrowedClosed, steps, event
vars == <<owned,borrowed,stopped,drain,ownedDrops,borrowedDrops,
          calledOwned,calledBorrowed,calls,borrowedClosed,steps,event>>
Init == /\ owned=0 /\ borrowed=0 /\ stopped=0 /\ drain=0
        /\ ownedDrops=0 /\ borrowedDrops=0 /\ calledOwned=0 /\ calledBorrowed=0
        /\ calls=0 /\ borrowedClosed=0 /\ steps=0 /\ event=0
AcceptOwned == /\ owned=0 /\ stopped=0 /\ owned'=1 /\ event'=1
               /\ UNCHANGED <<borrowed,stopped,calledOwned,calledBorrowed,calls>>
AcceptBorrowed == /\ borrowed=0 /\ stopped=0 /\ borrowed'=1 /\ event'=2
                  /\ UNCHANGED <<owned,stopped,calledOwned,calledBorrowed,calls>>
Drain == /\ stopped=0 /\ drain=0 /\ event'=3
         /\ UNCHANGED <<owned,borrowed,stopped,calledOwned,calledBorrowed,calls>>
CloseOwned == /\ owned=1 /\ owned'=2 /\ event'=4
              /\ UNCHANGED <<borrowed,stopped,calledOwned,calledBorrowed,calls>>
CancelBorrowed == /\ borrowed=1 /\ borrowed'=2 /\ event'=5
                  /\ UNCHANGED <<owned,stopped,calledOwned,calledBorrowed,calls>>
CallOwned == /\ owned=1 /\ calledOwned=0 /\ calledOwned'=1 /\ event'=6
             /\ calls'=calls+(IF Fault="loseCapability" THEN 0 ELSE 1)
             /\ UNCHANGED <<owned,borrowed,stopped,calledBorrowed>>
CallBorrowed == /\ borrowed=1 /\ calledBorrowed=0 /\ calledBorrowed'=1 /\ event'=7
                /\ calls'=calls+(IF Fault="loseCapability" THEN 0 ELSE 1)
                /\ UNCHANGED <<owned,borrowed,stopped,calledOwned>>
Stop == /\ stopped=0 /\ stopped'=1 /\ event'=8
        /\ owned'=(IF owned=1 THEN 2 ELSE owned)
        /\ borrowed'=(IF Fault="cancelBorrowed" /\ borrowed=1 THEN 2 ELSE borrowed)
        /\ UNCHANGED <<calledOwned,calledBorrowed,calls>>
PeerCloseBorrowed == /\ borrowed=1 /\ borrowed'=2 /\ event'=9
                     /\ UNCHANGED <<owned,stopped,calledOwned,calledBorrowed,calls>>
Observe ==
 /\ ownedDrops'=IF Fault="leakOwned" THEN 0 ELSE IF owned'=2 THEN 1 ELSE 0
 /\ borrowedDrops'=IF Fault="destroyBorrowed" /\ borrowed'=2 THEN 1 ELSE 0
 /\ drain'=IF drain\in {2,3} THEN drain
            ELSE IF event'=8 /\ drain=1 THEN 3
            ELSE IF drain=1 \/ event'=3
                 THEN IF Fault="earlyDrain" \/ (owned'#1 /\ (Fault#"countBorrowed" \/ borrowed'#1)) THEN 2 ELSE 1
            ELSE drain
Next == /\ steps<5 /\ steps'=steps+1
        /\ (AcceptOwned \/ AcceptBorrowed \/ Drain \/ CloseOwned \/ CancelBorrowed
            \/ CallOwned \/ CallBorrowed \/ Stop \/ PeerCloseBorrowed)
        /\ borrowedClosed'=(IF event'\in {5,9} THEN 1 ELSE borrowedClosed)
        /\ Observe
Spec == Init /\ [][Next]_vars
TypeOK == /\ owned\in 0..2 /\ borrowed\in 0..2 /\ stopped\in 0..1 /\ drain\in 0..3
          /\ ownedDrops\in 0..1 /\ borrowedDrops\in 0..1 /\ steps\in 0..5
          /\ calledOwned\in 0..1 /\ calledBorrowed\in 0..1 /\ borrowedClosed\in 0..1
          /\ calls\in 0..2 /\ event\in 0..9
Ownership == ownedDrops=(IF owned=2 THEN 1 ELSE 0) /\ borrowedDrops=0
DrainContract ==
 /\ (event=3 => drain=(IF owned=1 THEN 1 ELSE 2))
 /\ (drain=1 => owned=1 /\ stopped=0)
Capabilities == calls=calledOwned+calledBorrowed
BorrowedSurvivesOwner == borrowed=2 => borrowedClosed=1
=============================================================================
