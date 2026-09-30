----------------------- MODULE ComposedWireBoundary -----------------------
EXTENDS Naturals, Sequences
CONSTANTS Fault, MaxSteps, MaxRefs
P == INSTANCE CapnpNetwork WITH net <- 0, VatCount <- 2, IdCount <- 2,
     MaxOps <- 1, Plan <- <<>>, Bug <- Fault, AllowLoss <- FALSE,
     Introductions <- FALSE, ExpectSuccess <- FALSE
\* Scalar projection of the actual composed handlers, not a second handler
\* implementation. Two exports with bounded references. MaxSteps=0 folds the
\* step counter away, yielding a finite cyclic graph for long history replay;
\* it does not bound the number of calls or prove termination.
\* Release annotations are erased; Call identity pairing is retained while
\* caller method/data deliberately differ from their wire values. Each Call
\* starts with a fresh question, abstracting completed Return/Finish cleanup.
VARIABLES a,b,live,steps,event,id,amount,method,data,seenMethod,seenData,
          expectedA,expectedB,expectedLive
vars == <<a,b,live,steps,event,id,amount,method,data,seenMethod,seenData,
          expectedA,expectedB,expectedLive>>
Init == /\ a=0 /\ b=0 /\ live=0 /\ steps=0 /\ event=0 /\ id=0 /\ amount=0
        /\ method=0 /\ data=0 /\ seenMethod=0 /\ seenData=0
        /\ expectedA=0 /\ expectedB=0 /\ expectedLive=0
Configure(x,y) ==
    /\ event=0 /\ a'=x /\ b'=y /\ live'=1 /\ event'=1
    /\ expectedA'=x /\ expectedB'=y /\ expectedLive'=1
    /\ UNCHANGED <<steps,id,amount,method,data,seenMethod,seenData>>
Local == [P!InitialState EXCEPT
    !.exports[<<<<2,1>>,1>>]=[P!ExportEntry EXCEPT !.count=a, !.ref=P!Object(2,1), !.generation=1],
    !.exports[<<<<2,1>>,2>>]=[P!ExportEntry EXCEPT !.count=b, !.ref=P!Object(2,2), !.generation=1]]
Count(i) == CASE i=1->a [] i=2->b [] OTHER->0
CanStep == MaxSteps=0 \/ steps<MaxSteps
AfterStep == IF MaxSteps=0 THEN 0 ELSE steps+1
Release(i,n) ==
    /\ event>0 /\ live=1 /\ CanStep
    /\ event'=2 /\ steps'=AfterStep /\ id'=i /\ amount'=n
    /\ LET message==[P!Message EXCEPT !.kind="release", !.id=i, !.count=n]
           after==P!ReceiveRelease(Local,<<1,2>>,message)
           valid==Count(i)>0 /\ n<=Count(i)
       IN /\ a'=after.exports[<<<<2,1>>,1>>].count
          /\ b'=after.exports[<<<<2,1>>,2>>].count
          /\ live'=(IF <<1,2>> \in after.connected THEN 1 ELSE 0)
          /\ expectedA'=(IF ~valid THEN 0 ELSE a-(IF i=1 THEN n ELSE 0))
          /\ expectedB'=(IF ~valid THEN 0 ELSE b-(IF i=2 THEN n ELSE 0))
          /\ expectedLive'=(IF valid THEN 1 ELSE 0)
    /\ UNCHANGED <<method,data,seenMethod,seenData>>
Method(m) == IF m=0 THEN "echo" ELSE "factory"
Call(i,m,d) ==
    /\ event>0 /\ live=1 /\ CanStep /\ Count(i)>0
    /\ event'=3 /\ steps'=AfterStep /\ id'=i /\ method'=m /\ data'=d
    /\ LET asked==P!Ask(Local,<<1,2>>,"call",P!Target("importedCap",i,<<>>),<<>>,P!Flags,
                       Method(1-m),2,"caller",0,0,0,0,0,0,0)
           message==[Head(asked.wire[<<1,2>>]) EXCEPT !.method=Method(m), !.data=d]
           after==P!ReceiveRequest(asked,<<1,2>>,message)
       IN /\ seenMethod'=(IF after.ops[1].method="echo" THEN 0 ELSE 1)
          /\ seenData'=after.ops[1].data
    /\ UNCHANGED <<a,b,live,amount,expectedA,expectedB,expectedLive>>
Next == (\E x,y \in 1..MaxRefs: Configure(x,y))
        \/ (\E i \in 0..2,n \in 0..(MaxRefs+1): Release(i,n))
        \/ (\E i \in 1..2,m,d \in 0..1: Call(i,m,d))
Spec == Init /\ [][Next]_vars
TypeOK == /\ a\in 0..MaxRefs /\ b\in 0..MaxRefs /\ live\in 0..1 /\ steps\in 0..MaxSteps
          /\ event\in 0..3 /\ id\in 0..2 /\ amount\in 0..(MaxRefs+1)
          /\ method\in 0..1 /\ data\in 0..1 /\ seenMethod\in 0..1 /\ seenData\in 0..2
          /\ expectedA\in 0..MaxRefs /\ expectedB\in 0..MaxRefs /\ expectedLive\in 0..1
WireRelease == a=expectedA /\ b=expectedB /\ live=expectedLive
WireMethod == event=3 => seenMethod=method
WireData == event=3 => seenData=data
=============================================================================
