-------------------------- MODULE ShutdownWaiters --------------------------
EXTENDS Naturals
CONSTANT Fault
\* A and B observe the old control; C observes a distinct replacement control.
\* Status: 0 absent, 1 constructed, 2 pending, 3 observed, 4 canceled.
\* Local task polls and synchronous Control operations are atomic. Socket IO,
\* Notify internals and actual clock expiration are outside this model.
VARIABLES old, fresh, firstOld, firstNew, a, b, c, wa, wb, wc,
          seenA, seenB, seenC, replaced, startedB, event
vars == <<old,fresh,firstOld,firstNew,a,b,c,wa,wb,wc,seenA,seenB,seenC,replaced,startedB,event>>
Init == /\ old=0 /\ fresh=0 /\ firstOld=0 /\ firstNew=0
        /\ a=0 /\ b=0 /\ c=0 /\ wa=0 /\ wb=0 /\ wc=0
        /\ seenA=0 /\ seenB=0 /\ seenC=0 /\ replaced=0 /\ startedB=0 /\ event=0
StartA == /\ a=0 \/ (a=4 /\ replaced=0)
          /\ a'=1 /\ wa'=0 /\ replaced'=(IF a=4 THEN 1 ELSE replaced) /\ event'=1
          /\ UNCHANGED <<old,fresh,firstOld,firstNew,b,c,wb,wc,seenA,seenB,seenC,startedB>>
PollA == /\ a=1 \/ (a=2 /\ wa=1)
         /\ a'=(IF old=0 THEN 2 ELSE 3) /\ seenA'=old /\ wa'=0 /\ event'=2
         /\ UNCHANGED <<old,fresh,firstOld,firstNew,b,c,wb,wc,seenB,seenC,replaced,startedB>>
CancelA == /\ a\in {1,2} /\ replaced=0 /\ a'=4 /\ wa'=0 /\ event'=3
           /\ b'=(IF Fault="cancelSibling" THEN 0 ELSE b)
           /\ UNCHANGED <<old,fresh,firstOld,firstNew,c,wb,wc,seenA,seenB,seenC,replaced,startedB>>
StartB == /\ b=0 /\ startedB=0 /\ b'=1 /\ startedB'=1 /\ event'=4
          /\ UNCHANGED <<old,fresh,firstOld,firstNew,a,c,wa,wb,wc,seenA,seenB,seenC,replaced>>
PollB == /\ b=1 \/ (b=2 /\ wb=1)
         /\ b'=(IF old=0 THEN 2 ELSE 3) /\ seenB'=old /\ wb'=0 /\ event'=5
         /\ UNCHANGED <<old,fresh,firstOld,firstNew,a,c,wa,wc,seenA,seenC,replaced,startedB>>
StartC == /\ c=0 /\ c'=1 /\ event'=7
          /\ UNCHANGED <<old,fresh,firstOld,firstNew,a,b,wa,wb,wc,seenA,seenB,seenC,replaced,startedB>>
PollC == /\ c=1 \/ (c=2 /\ wc=1)
         /\ c'=(IF fresh=0 THEN 2 ELSE 3) /\ seenC'=fresh /\ wc'=0 /\ event'=8
         /\ UNCHANGED <<old,fresh,firstOld,firstNew,a,b,wa,wb,seenA,seenB,replaced,startedB>>
FinishOld(kind) ==
    /\ firstOld'=(IF firstOld=0 THEN kind ELSE firstOld)
    /\ old'=(IF old=0 \/ Fault="overwrite" THEN kind ELSE old)
    /\ wa'=(IF a=2 THEN 1 ELSE wa)
    /\ wb'=(IF b=2 /\ Fault#"wakeOne" THEN 1 ELSE wb)
    /\ fresh'=(IF Fault="crossGeneration" THEN kind ELSE fresh)
    /\ event'=9+kind
    /\ UNCHANGED <<firstNew,a,b,c,wc,seenA,seenB,seenC,replaced,startedB>>
FinishNew == /\ fresh'=4 /\ firstNew'=4 /\ wc'=(IF c=2 THEN 1 ELSE wc) /\ event'=13
             /\ UNCHANGED <<old,firstOld,a,b,c,wa,wb,seenA,seenB,seenC,replaced,startedB>>
Next == StartA \/ PollA \/ CancelA \/ StartB \/ PollB \/ StartC \/ PollC
        \/ (\E kind\in 1..3: FinishOld(kind)) \/ FinishNew
Spec == Init /\ [][Next]_vars
FairSpec == Spec /\ WF_vars(PollA) /\ WF_vars(PollB) /\ WF_vars(PollC)
TypeOK == /\ old\in 0..3 /\ fresh\in 0..4 /\ firstOld\in 0..3 /\ firstNew\in {0,4}
          /\ a\in 0..4 /\ b\in 0..3 /\ c\in 0..3 /\ wa\in 0..1 /\ wb\in 0..1 /\ wc\in 0..1
          /\ seenA\in 0..3 /\ seenB\in 0..3 /\ seenC\in 0..4 /\ replaced\in 0..1
          /\ startedB\in 0..1 /\ event\in 0..13
FirstOutcome == old=firstOld /\ fresh=firstNew
Broadcast == /\ (old#0 /\ a=2 => wa=1) /\ (old#0 /\ b=2 => wb=1)
             /\ (fresh#0 /\ c=2 => wc=1)
CancellationIsolation == startedB=1 => b#0
Observation == /\ (a=3 => seenA=firstOld /\ seenA#0)
               /\ (b=3 => seenB=firstOld /\ seenB#0)
               /\ (c=3 => seenC=4)
Progress == /\ (a=2 /\ old#0) ~> (a\in {3,4})
            /\ (b=2 /\ old#0) ~> (b=3)
            /\ (c=2 /\ fresh#0) ~> (c=3)
=============================================================================
