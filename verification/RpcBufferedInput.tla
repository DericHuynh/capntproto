------------------------- MODULE RpcBufferedInput -------------------------
EXTENDS Naturals
CONSTANT Fault
\* Two frames (3/4 words) fit in the receive buffer. Input arrives one word
\* at a time; each read can prefetch all available words. Pending reads are
\* canceled immediately and retried, so their prefix must survive cancellation.
\* h: 0 absent/released, 1 shared-buffer lease, 2 independent message.
\* result: 0 none, 1 message, 2 pending/canceled, 3 EOF, 4 error, 5 live lease.
VARIABLES offered,pos,delivered,h1,h2,ended,terminal,failed,kind,result,damaged,lost,event
vars == <<offered,pos,delivered,h1,h2,ended,terminal,failed,kind,result,damaged,lost,event>>
Boundary(n) == IF n=0 THEN 0 ELSE IF n=1 THEN 3 ELSE 7
Leased == h1=1 \/ h2=1
Init == /\ offered=0 /\ pos=0 /\ delivered=0 /\ h1=0 /\ h2=0
        /\ ended=0 /\ terminal=0 /\ failed=0 /\ kind=0 /\ result=0
        /\ damaged=0 /\ lost=0 /\ event=0
Offer == /\ ended=0 /\ offered<7 /\ offered'=offered+1 /\ event'=1
         /\ UNCHANGED <<pos,delivered,h1,h2,ended,terminal,failed,kind,result,damaged,lost>>
End == /\ ended=0 /\ ended'=1 /\ event'=2
       /\ UNCHANGED <<offered,pos,delivered,h1,h2,terminal,failed,kind,result,damaged,lost>>
Read(short) ==
    LET next == Boundary(delivered+1)
        ready == delivered<2 /\ pos>=next
        readPos == IF Leased \/ terminal#0 \/ ready THEN pos ELSE offered
        complete == delivered<2 /\ readPos>=next
        outcome == IF Leased THEN 5 ELSE IF terminal=2 THEN 4
                   ELSE IF terminal=1 THEN 3 ELSE IF complete THEN 1
                   ELSE IF ended=0 THEN 2 ELSE IF readPos=Boundary(delivered) THEN 3 ELSE 4
        mode == IF short=1 \/ Fault="retainShared" THEN 1 ELSE 2
    IN /\ kind'=short /\ result'=outcome /\ event'=3 /\ pos'=readPos
       /\ delivered'=IF outcome=1 THEN delivered+1 ELSE delivered
       /\ h1'=IF outcome=1 /\ delivered=0 THEN mode ELSE h1
       /\ h2'=IF outcome=1 /\ delivered=1 THEN mode ELSE h2
       /\ terminal'=IF terminal=2 /\ Fault="retryFailure" THEN 0
                    ELSE IF outcome=4 THEN 2 ELSE IF outcome=3 THEN 1 ELSE terminal
       /\ failed'=IF outcome=4 THEN 1 ELSE failed
       /\ damaged'=IF Leased /\ Fault="overwriteLive" THEN 1 ELSE damaged
       /\ lost'=IF outcome=2 /\ readPos>Boundary(delivered) /\ Fault="cancelPrefix" THEN 1 ELSE lost
       /\ UNCHANGED <<offered,ended>>
Release(first) == /\ (IF first THEN h1#0 ELSE h2#0)
                  /\ h1'=IF first THEN 0 ELSE h1
                  /\ h2'=IF first THEN h2 ELSE 0
                  /\ event'=IF first THEN 4 ELSE 5
                  /\ UNCHANGED <<offered,pos,delivered,ended,terminal,failed,kind,result,damaged,lost>>
ReadNext == Read(0) \/ Read(1)
ReleaseNext == Release(TRUE) \/ Release(FALSE)
Next == Offer \/ End \/ ReadNext \/ ReleaseNext
Spec == Init /\ [][Next]_vars
LiveSpec == Spec /\ WF_vars(ReadNext) /\ WF_vars(ReleaseNext)
EndProgress == ended=1 ~> terminal#0
TypeOK == /\ offered\in 0..7 /\ pos\in 0..7 /\ delivered\in 0..2
          /\ h1\in 0..2 /\ h2\in 0..2 /\ ended\in 0..1 /\ terminal\in 0..2
          /\ failed\in 0..1 /\ kind\in 0..1 /\ result\in 0..5
          /\ damaged\in 0..1 /\ lost\in 0..1 /\ event\in 0..5
Cursor == Boundary(delivered)<=pos /\ pos<=offered
LeaseSafety == damaged=0 /\ ~(h1=1 /\ h2#0)
PrefixSurvivesCancel == lost=0
IndependentRetention == event=3 /\ result=1 /\ kind=0 =>
                         (IF delivered=1 THEN h1=2 ELSE h2=2)
StickyFailure == failed=1 => terminal=2
=============================================================================
