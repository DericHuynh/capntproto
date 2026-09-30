-------------------------- MODULE BulkReservation --------------------------
EXTENDS Integers
CONSTANT Fault
\* Two independent two-byte windows, two issued sequences per window, and one
\* retained reservation per window. Dropped reservations leave outstanding debt.
\* Events 1..4 reserve one/two bytes; 5..6 settle locally; 7..8 settle in the
\* other window; 9..10 discard a handle. A result is: backpressure=0, reserved=1,
\* released=2, already-settled=3, wrong-window=4, exhausted=5, discarded=6.
VARIABLES ac, bc, an, bn, a1, a2, b1, b2, ah, bh, az, bz, event, result, priorA, priorB
vars == <<ac,bc,an,bn,a1,a2,b1,b2,ah,bh,az,bz,event,result,priorA,priorB>>
Init == /\ ac=2 /\ bc=2 /\ an=1 /\ bn=1
        /\ a1=0 /\ a2=0 /\ b1=0 /\ b2=0 /\ ah=0 /\ bh=0 /\ az=0 /\ bz=0
        /\ event=0 /\ result=0 /\ priorA=2 /\ priorB=2
Reserve(owner, bytes) ==
    /\ IF owner=1 THEN ah=0 ELSE bh=0
    /\ LET next == IF owner=1 THEN an ELSE bn
           credit == IF owner=1 THEN ac ELSE bc
           exhausted == next>2 /\ Fault#"exhaust"
           success == ~exhausted /\ credit>=bytes
           charge == IF success THEN bytes ELSE
                         IF ~exhausted /\ Fault="backpressure" /\ credit=1 THEN 1 ELSE 0
       IN /\ result'=(IF exhausted THEN 5 ELSE IF success THEN 1 ELSE 0)
          /\ ac'=(IF owner=1 THEN ac-charge ELSE ac)
          /\ bc'=(IF owner=2 THEN bc-charge ELSE bc)
          /\ an'=(IF owner=1 /\ success THEN an+1 ELSE an)
          /\ bn'=(IF owner=2 /\ success THEN bn+1 ELSE bn)
          /\ ah'=(IF owner=1 /\ success THEN an ELSE ah)
          /\ bh'=(IF owner=2 /\ success THEN bn ELSE bh)
          /\ az'=(IF owner=1 /\ success THEN bytes ELSE az)
          /\ bz'=(IF owner=2 /\ success THEN bytes ELSE bz)
          /\ a1'=(IF owner=1 /\ success /\ an=1 THEN bytes ELSE a1)
          /\ a2'=(IF owner=1 /\ success /\ an=2 THEN bytes ELSE a2)
          /\ b1'=(IF owner=2 /\ success /\ bn=1 THEN bytes ELSE b1)
          /\ b2'=(IF owner=2 /\ success /\ bn=2 THEN bytes ELSE b2)
    /\ event'=2*(owner-1)+bytes /\ priorA'=ac /\ priorB'=bc
Settle(owner, target) ==
    /\ IF owner=1 THEN ah#0 ELSE bh#0
    /\ LET id == IF owner=1 THEN ah ELSE bh
           valid == owner=target \/ Fault="foreign"
           pending == IF target=1 THEN (IF id=1 THEN a1 ELSE a2)
                      ELSE (IF id=1 THEN b1 ELSE b2)
           credit == IF ~valid THEN 0 ELSE IF pending#0 THEN pending ELSE
                       IF Fault="duplicate" THEN (IF owner=1 THEN az ELSE bz) ELSE 0
       IN /\ ac'=(IF target=1 THEN ac+credit ELSE ac)
          /\ bc'=(IF target=2 THEN bc+credit ELSE bc)
          /\ a1'=(IF valid /\ target=1 /\ id=1 THEN 0 ELSE a1)
          /\ a2'=(IF valid /\ target=1 /\ id=2 THEN 0 ELSE a2)
          /\ b1'=(IF valid /\ target=2 /\ id=1 THEN 0 ELSE b1)
          /\ b2'=(IF valid /\ target=2 /\ id=2 THEN 0 ELSE b2)
          /\ result'=(IF ~valid THEN 4 ELSE IF pending=0 THEN 3 ELSE 2)
    /\ event'=(IF owner=target THEN 4+owner ELSE 6+owner)
    /\ priorA'=ac /\ priorB'=bc
    /\ UNCHANGED <<an,bn,ah,bh,az,bz>>
Discard(owner) ==
    /\ IF owner=1 THEN ah#0 ELSE bh#0
    /\ ac'=(IF owner=1 /\ Fault="drop" THEN ac+(IF ah=1 THEN a1 ELSE a2) ELSE ac)
    /\ bc'=(IF owner=2 /\ Fault="drop" THEN bc+(IF bh=1 THEN b1 ELSE b2) ELSE bc)
    /\ ah'=(IF owner=1 THEN 0 ELSE ah) /\ bh'=(IF owner=2 THEN 0 ELSE bh)
    /\ az'=(IF owner=1 THEN 0 ELSE az) /\ bz'=(IF owner=2 THEN 0 ELSE bz)
    /\ event'=8+owner /\ result'=6 /\ priorA'=ac /\ priorB'=bc
    /\ UNCHANGED <<an,bn,a1,a2,b1,b2>>
Next == (\E owner\in 1..2, bytes\in 1..2: Reserve(owner,bytes))
        \/ (\E owner\in 1..2, target\in 1..2: Settle(owner,target))
        \/ (\E owner\in 1..2: Discard(owner))
Spec == Init /\ [][Next]_vars
TypeOK == /\ ac\in 0..4 /\ bc\in 0..4 /\ an\in 1..4 /\ bn\in 1..4
          /\ a1\in 0..2 /\ a2\in 0..2 /\ b1\in 0..2 /\ b2\in 0..2
          /\ ah\in 0..3 /\ bh\in 0..3 /\ az\in 0..2 /\ bz\in 0..2
          /\ event\in 0..10 /\ result\in 0..6 /\ priorA\in 0..4 /\ priorB\in 0..4
Namespace == an<=3 /\ bn<=3
ForeignIsInert == event\in {7,8} => ac=priorA /\ bc=priorB /\ result=4
DropIsInert == event\in {9,10} => ac=priorA /\ bc=priorB
Conservation == ac+a1+a2=2 /\ bc+b1+b2=2
BoundedCredit == ac<=2 /\ bc<=2
=============================================================================
