------------------------- MODULE NoiseClockDomains -------------------------
EXTENDS Naturals
CONSTANT Fault
\* One tick is five seconds. The original connection starts at 0 and a
\* replacement, if created, starts at 1. Both negotiate ten-second idle expiry.
\* Polling a timer is an adapter action: time alone does not execute callbacks.
VARIABLES now, replacement, oldClosed, newClosed, oldPoll, newPoll, event
vars == <<now,replacement,oldClosed,newClosed,oldPoll,newPoll,event>>
Init == /\ now=0 /\ replacement=0 /\ oldClosed=0 /\ newClosed=0
        /\ oldPoll=0 /\ newPoll=0 /\ event=0
Tick == /\ now<3 /\ now'=now+1 /\ event'=1
        /\ UNCHANGED <<replacement,oldClosed,newClosed,oldPoll,newPoll>>
Replace == /\ now=1 /\ replacement=0 /\ replacement'=1 /\ event'=2
           /\ UNCHANGED <<now,oldClosed,newClosed,oldPoll,newPoll>>
ReadTime == IF Fault="wallClock" THEN 0 ELSE now
PollOld == /\ oldPoll#now+1 /\ oldPoll'=now+1 /\ event'=3
           /\ oldClosed'=IF ReadTime >= (IF Fault="early" THEN 1 ELSE 2)
                         THEN 1 ELSE 0
           /\ UNCHANGED <<now,replacement,newClosed,newPoll>>
PollNew == /\ replacement=1 /\ newPoll#now+1 /\ newPoll'=now+1 /\ event'=4
           /\ newClosed'=IF ReadTime >= (IF Fault="inherit" THEN 2 ELSE 3)
                         THEN 1 ELSE 0
           /\ UNCHANGED <<now,replacement,oldClosed,oldPoll>>
Next == Tick \/ Replace \/ PollOld \/ PollNew
Spec == Init /\ [][Next]_vars
TypeOK == /\ now\in 0..3 /\ replacement\in 0..1
          /\ oldClosed\in 0..1 /\ newClosed\in 0..1
          /\ oldPoll\in 0..4 /\ newPoll\in 0..4 /\ event\in 0..4
OldDeadline == event=3 => oldClosed=(IF now>=2 THEN 1 ELSE 0)
NewDeadline == event=4 => newClosed=(IF now>=3 THEN 1 ELSE 0)
=============================================================================
