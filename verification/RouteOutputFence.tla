-------------------------- MODULE RouteOutputFence --------------------------
EXTENDS Naturals
CONSTANTS Fault, ErrorAt
\* One terminated batch (two messages), a retained connection, and the actual
\* route writer watcher. Gates model write/flush/close readiness. ErrorAt picks
\* no error (0), write (1), flush (2), or close (3) failure.
\* Phase: writing=0, flushing=1, closing=2, awaiting disconnect=3, done=4,
\* canceled=5. Fence: pending=0, success=1, IO error=2, driver canceled=3.
\* This model stops at local output closure, not an authenticated peer receipt.
VARIABLES writeGate, flushGate, closeGate, phase, wrote, flushed, closed,
          marker, fence, draining, stopped, disconnected, cause, first,
          canceledPoll, event
vars == <<writeGate,flushGate,closeGate,phase,wrote,flushed,closed,marker,
          fence,draining,stopped,disconnected,cause,first,canceledPoll,event>>
Init == /\ writeGate=0 /\ flushGate=0 /\ closeGate=0 /\ phase=0
        /\ wrote=0 /\ flushed=0 /\ closed=0 /\ marker=0 /\ fence=0
        /\ draining=0 /\ stopped=0 /\ disconnected=0 /\ cause=0 /\ first=0
        /\ canceledPoll=0 /\ event=0
ReleaseWrite == /\ writeGate=0 /\ writeGate'=1 /\ event'=1
                /\ UNCHANGED <<flushGate,closeGate,phase,wrote,flushed,closed,
                    marker,fence,draining,stopped,disconnected,cause,first,canceledPoll>>
ReleaseFlush == /\ flushGate=0 /\ flushGate'=1 /\ event'=2
                /\ UNCHANGED <<writeGate,closeGate,phase,wrote,flushed,closed,
                    marker,fence,draining,stopped,disconnected,cause,first,canceledPoll>>
ReleaseClose == /\ closeGate=0 /\ closeGate'=1 /\ event'=3
                /\ UNCHANGED <<writeGate,flushGate,phase,wrote,flushed,closed,
                    marker,fence,draining,stopped,disconnected,cause,first,canceledPoll>>
PollWorker ==
    /\ phase<4
    /\ LET abort == stopped=1 /\ Fault#"missCancel"
           pw == IF phase=0 /\ writeGate=1 THEN (IF ErrorAt=1 THEN 2 ELSE 1) ELSE phase
           pf == IF pw=1 /\ (flushGate=1 \/ Fault="skipFlush") THEN 2 ELSE pw
           pc == IF pf=2 /\ closeGate=1 THEN 3 ELSE pf
           out == IF pc>=3 \/ (pf=2 /\ Fault="earlyFence")
                  THEN (IF ErrorAt=0 THEN 1 ELSE 2) ELSE fence
           report == ~abort /\ out=2 /\ (draining=0 \/ Fault="drainFailure") /\ cause=0
       IN /\ phase'=(IF abort THEN 5 ELSE IF pc=3 /\ disconnected=1 THEN 4 ELSE pc)
          /\ wrote'=(IF ~abort /\ phase=0 /\ writeGate=1 /\ ErrorAt#1 THEN 1 ELSE wrote)
          /\ flushed'=(IF ~abort /\ pw=1 /\ flushGate=1 /\ ErrorAt#2 THEN 1 ELSE flushed)
          /\ closed'=(IF ~abort /\ pc>=3 THEN 1 ELSE closed)
          /\ marker'=(IF marker#0 THEN marker ELSE IF abort THEN 2
                       ELSE IF pf>=2 THEN (IF ErrorAt\in {1,2} THEN 2 ELSE 1) ELSE 0)
          /\ fence'=(IF abort THEN (IF fence=0 THEN 3 ELSE fence) ELSE out)
          /\ cause'=(IF report THEN 1 ELSE cause)
          /\ first'=(IF report THEN 1 ELSE first)
    /\ canceledPoll'=(IF stopped=1 THEN 1 ELSE canceledPoll) /\ event'=4
    /\ UNCHANGED <<writeGate,flushGate,closeGate,draining,stopped,disconnected>>
Drain == /\ draining=0 /\ cause=0 /\ draining'=1 /\ event'=5
         /\ UNCHANGED <<writeGate,flushGate,closeGate,phase,wrote,flushed,closed,
                        marker,fence,stopped,disconnected,cause,first,canceledPoll>>
Stop == /\ stopped=0 /\ stopped'=1
        /\ cause'=(IF cause=0 \/ Fault="overwrite" THEN 2 ELSE cause)
        /\ first'=(IF first=0 THEN 2 ELSE first) /\ event'=6
        /\ UNCHANGED <<writeGate,flushGate,closeGate,phase,wrote,flushed,closed,
                       marker,fence,draining,disconnected,canceledPoll>>
Disconnect == /\ disconnected=0 /\ disconnected'=1 /\ event'=7
              /\ UNCHANGED <<writeGate,flushGate,closeGate,phase,wrote,flushed,closed,
                  marker,fence,draining,stopped,cause,first,canceledPoll>>
Next == ReleaseWrite \/ ReleaseFlush \/ ReleaseClose \/ PollWorker \/ Drain \/ Stop \/ Disconnect
Spec == Init /\ [][Next]_vars
FairSpec == Spec /\ WF_vars(PollWorker)
TypeOK == /\ \A x\in {writeGate,flushGate,closeGate,wrote,flushed,closed,draining,
                       stopped,disconnected,canceledPoll}: x\in 0..1
          /\ phase\in 0..5 /\ marker\in 0..2 /\ fence\in 0..3
          /\ cause\in 0..2 /\ first\in 0..2 /\ event\in 0..7
MarkerFence == marker=1 => wrote=1 /\ flushed=1
OutputPublication == fence\in {1,2} => closed=1
Success == fence=1 => wrote=1 /\ flushed=1 /\ ErrorAt=0
FirstCause == cause=first
DrainOwnsCause == draining=1 /\ stopped=0 => cause=0
CanceledPoll == canceledPoll=1 => phase=5
DriverWaits == phase=4 => disconnected=1 /\ fence\in {1,2}
Progress == /\ (writeGate=1 /\ flushGate=1 /\ closeGate=1) ~> (fence#0)
            /\ (stopped=1) ~> (phase>=4)
=============================================================================
