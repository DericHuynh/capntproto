------------------------- MODULE RpcAdaptiveWindow -------------------------
EXTENDS Naturals
CONSTANTS Fault, Scenario, MaxRounds
\* Two sends per batch, immediate delivery to the output queue. Acknowledgements
\* may arrive in either order. Scenario 0 varies ack timing with saturated
\* samples; 1 uses small app-limited messages through startup exit; 2 has zero
\* initial credit and zero ack intervals. Units are bytes and microseconds.
\* Failed acknowledgements/drop/drain are modeled in RpcVariableWindow and
\* RpcStreaming; the shared Rust lifetime code also has adaptive native tests.
VARIABLES window,delivered,lastTime,firstTime,firstDelivered,minRtt,startup,plateau,
          roundStart,roundSet,lastWindow,now,rounds,pending,a1,a2,p1,p2,sentTime,
          sentWindow,baseTime,baseDelivered,event,whichAck,estimated,limited,oldWindow,sampleStartup
vars == <<window,delivered,lastTime,firstTime,firstDelivered,minRtt,startup,plateau,
          roundStart,roundSet,lastWindow,now,rounds,pending,a1,a2,p1,p2,sentTime,
          sentWindow,baseTime,baseDelivered,event,whichAck,estimated,limited,oldWindow,sampleStartup>>
Min(a,b) == IF a<b THEN a ELSE b
Max(a,b) == IF a>b THEN a ELSE b
Clamp(w) == Max(65536,Min(1073741824,w))
Growth(w,s) == IF s=1 THEN w*2 ELSE (w*5) \div 4
AppliedGrowth(w,s) == IF Fault="steadyGrowth" THEN w*2 ELSE Growth(w,s)
Decay(w) == (w*7) \div 8
Size(r) == IF Scenario=1 /\ r<5 THEN 8 ELSE 524288
Chunk == Size(rounds)
InitialWindow == IF Scenario=2 THEN 0 ELSE 262144
Ready(bytes,w) == bytes < w+Chunk
Delays == IF Scenario=0 THEN {0,1,4} ELSE IF Scenario=1 THEN {1} ELSE {0}
Init == /\ window=InitialWindow /\ delivered=0 /\ lastTime=0 /\ firstTime=0
        /\ firstDelivered=0 /\ minRtt=100 /\ startup=1 /\ plateau=0
        /\ roundStart=0 /\ roundSet=0 /\ lastWindow=0 /\ now=0 /\ rounds=0
        /\ pending=0 /\ a1=0 /\ a2=0 /\ p1=0 /\ p2=0 /\ sentTime=0
        /\ sentWindow=InitialWindow /\ baseTime=0 /\ baseDelivered=0
        /\ event=0 /\ whichAck=0 /\ estimated=0 /\ limited=0 /\ oldWindow=InitialWindow
        /\ sampleStartup=1
SendBatch ==
    /\ pending=0 /\ rounds<MaxRounds /\ rounds'=rounds+1 /\ event'=1
    /\ now'=now+1 /\ sentTime'=now+1 /\ sentWindow'=window
    /\ baseTime'=lastTime /\ baseDelivered'=delivered
    /\ pending'=2 /\ a1'=0 /\ a2'=0
    /\ p1'=IF Size(rounds+1)<window+Size(rounds+1) THEN 1 ELSE 2
    /\ p2'=IF 2*Size(rounds+1)<window+Size(rounds+1) THEN 1 ELSE 2
    /\ estimated'=0
    /\ UNCHANGED <<window,delivered,lastTime,firstTime,firstDelivered,minRtt,startup,
                   plateau,roundStart,roundSet,lastWindow,whichAck,limited,oldWindow,sampleStartup>>
Ack(i,d) ==
    /\ rounds>0 /\ (IF i=1 THEN a1=0 ELSE a2=0)
    /\ event'=2 /\ whichAck'=i /\ now'=now+d /\ pending'=pending-1
    /\ a1'=IF i=1 THEN 1 ELSE a1
    /\ a2'=IF i=2 THEN 1 ELSE a2
    /\ delivered'=delivered+Chunk /\ lastTime'=now'
    /\ minRtt'=Min(minRtt,now'-sentTime)
    /\ firstTime'=IF firstDelivered=0 THEN now' ELSE firstTime
    /\ firstDelivered'=IF firstDelivered=0 THEN delivered' ELSE firstDelivered
    /\ oldWindow'=window
    /\ sampleStartup'=startup
    /\ limited'=IF Ready(i*Chunk,sentWindow) THEN 1 ELSE 0
    /\ LET bt == IF baseDelivered=0 THEN firstTime ELSE baseTime
           bd == IF baseDelivered=0 THEN firstDelivered ELSE baseDelivered
           interval == now'-bt
           sample == firstDelivered#0 /\ interval>0
           estimate == IF sample THEN AppliedGrowth((delivered'-bd)*minRtt',startup) \div interval ELSE 0
           upper == AppliedGrowth(sentWindow,startup)
           lower == IF limited'=1 THEN (IF Fault="shrinkLimited" THEN 0 ELSE window)
                    ELSE IF Fault="skipDecay" THEN 0 ELSE Decay(sentWindow)
           next == IF sample THEN Clamp(Max(lower,IF Fault="unboundedGrowth" THEN estimate ELSE Min(estimate,upper))) ELSE window
           newRound == sample /\ startup=1 /\ (roundSet=0 \/ sentTime>=roundStart)
           stalled == IF next>Growth(lastWindow,0) THEN 0 ELSE plateau+1
           Release(p) == IF p=2 /\ Ready(pending'*Chunk,next) /\ Fault#"dropCredit" THEN 1 ELSE p
       IN /\ estimated'=IF sample THEN 1 ELSE 0
          /\ window'=next
          /\ plateau'=IF newRound THEN stalled ELSE plateau
          /\ startup'=IF newRound /\ stalled>=3 /\ Fault#"neverExit" THEN 0 ELSE startup
          /\ roundStart'=IF newRound THEN now' ELSE roundStart
          /\ roundSet'=IF newRound THEN 1 ELSE roundSet
          /\ lastWindow'=IF newRound THEN next ELSE lastWindow
          /\ p1'=Release(p1) /\ p2'=Release(p2)
    /\ UNCHANGED <<rounds,sentTime,sentWindow,baseTime,baseDelivered>>
AckNext == \E i\in {1,2}, d\in Delays: Ack(i,d)
Next == SendBatch \/ AckNext
Spec == Init /\ [][Next]_vars
LiveSpec == Spec /\ WF_vars(AckNext)
Progress == pending>0 ~> pending=0
TypeOK == /\ window\in Nat /\ delivered\in Nat /\ pending\in 0..2 /\ rounds\in 0..MaxRounds
          /\ startup\in 0..1 /\ plateau\in 0..(2*MaxRounds) /\ estimated\in 0..1
          /\ a1\in 0..1 /\ a2\in 0..1 /\ p1\in 0..2 /\ p2\in 0..2
          /\ minRtt\in 0..100 /\ event\in 0..2
WindowBounds == estimated=1 => window>=65536 /\ window<=1073741824
AppLimited == estimated=1 /\ limited=1 => window>=Clamp(oldWindow)
GrowthCollar == estimated=1 => window<=Clamp(Max(Growth(sentWindow,sampleStartup),IF limited=1 THEN oldWindow ELSE 0))
DecayCollar == estimated=1 /\ limited=0 => window>=Clamp(Decay(sentWindow))
StartupExit == plateau>=3 => startup=0
ReadyCredit == event=2 /\ Ready(pending*Chunk,window) => p1=1 /\ p2=1
=============================================================================
