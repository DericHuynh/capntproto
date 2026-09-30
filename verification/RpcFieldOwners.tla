------------------------ MODULE RpcFieldOwners ------------------------
EXTENDS Naturals, TLC
CONSTANTS RootCost, ReadCost, Fault
Capacity == 3 * ReadCost
VARIABLES phase, budget, spent, moved, detached, extracted, held, copyTried,
          copied, copyOk, readFailed, probed, unauthorized, identity,
          storageAlive, contextAlive, capAlive, event
vars == <<phase,budget,spent,moved,detached,extracted,held,copyTried,copied,
          copyOk,readFailed,probed,unauthorized,identity,storageAlive,contextAlive,capAlive,event>>
(* phase: 0 dropped, 1 mutable, 2 frozen, 3 reader, 4 extracted reader+context *)
Init == /\ phase=1 /\ budget=0 /\ spent=0 /\ identity=1
        /\ moved=FALSE /\ detached=FALSE /\ extracted=FALSE /\ held=FALSE
        /\ copyTried=FALSE /\ copied=FALSE /\ copyOk=FALSE
        /\ readFailed=FALSE /\ probed=FALSE /\ unauthorized=FALSE
        /\ storageAlive=TRUE /\ contextAlive=TRUE /\ capAlive=TRUE /\ event=0
Freeze == /\ phase=1 /\ phase'=2 /\ event'=1
          /\ UNCHANGED <<budget,spent,moved,detached,extracted,held,copyTried,copied,copyOk,
                         readFailed,probed,unauthorized,identity>>
Reader == /\ phase\in {1,2} /\ phase'=3 /\ budget'=Capacity /\ spent'=0
          /\ identity'=(IF Fault="context" THEN 2 ELSE identity) /\ event'=2
          /\ UNCHANGED <<moved,detached,extracted,held,copyTried,copied,copyOk,
                         readFailed,probed,unauthorized>>
Move == /\ phase>0 /\ ~moved /\ moved'=TRUE /\ event'=3
        /\ budget'=IF Fault="resetMove" /\ phase\in {3,4} THEN Capacity ELSE budget
        /\ UNCHANGED <<phase,spent,detached,extracted,held,copyTried,copied,copyOk,
                       readFailed,probed,unauthorized,identity>>
Detach == /\ phase=3 /\ ~detached /\ detached'=TRUE /\ phase'=4 /\ event'=4
          /\ UNCHANGED <<budget,spent,moved,extracted,held,copyTried,copied,copyOk,
                         readFailed,probed,unauthorized,identity>>
Reattach == /\ phase=4 /\ event'=5
            /\ phase'=IF budget>=RootCost \/ Fault="resetRebind" THEN 3 ELSE 0
            /\ budget'=IF Fault="resetRebind" THEN Capacity
                       ELSE IF budget>=RootCost THEN budget-RootCost ELSE 0
            /\ spent'=spent+RootCost
            /\ UNCHANGED <<moved,detached,extracted,held,copyTried,copied,copyOk,
                           readFailed,probed,unauthorized,identity>>
Read == /\ phase=3 /\ budget>=ReadCost /\ budget'=budget-ReadCost
        /\ spent'=spent+ReadCost /\ event'=6
        /\ UNCHANGED <<phase,moved,detached,extracted,held,copyTried,copied,copyOk,
                       readFailed,probed,unauthorized,identity>>
FailRead == /\ phase=3 /\ budget<ReadCost /\ ~readFailed /\ readFailed'=TRUE /\ event'=7
            /\ UNCHANGED <<phase,budget,spent,moved,detached,extracted,held,copyTried,copied,
                           copyOk,probed,unauthorized,identity>>
Extract == /\ (phase=3 \/ copied) /\ ~extracted /\ extracted'=TRUE /\ held'=TRUE /\ event'=8
           /\ UNCHANGED <<phase,budget,spent,moved,detached,copyTried,copied,copyOk,
                          readFailed,probed,unauthorized,identity>>
Copy == /\ phase=3 /\ ~copyTried /\ copyTried'=TRUE /\ event'=9
        /\ copyOk'=(budget>=ReadCost)
        /\ copied'=(budget>=ReadCost \/ Fault="partialCopy")
        /\ budget'=IF budget>=ReadCost THEN budget-ReadCost ELSE budget
        /\ spent'=IF budget>=ReadCost THEN spent+ReadCost ELSE spent
        /\ UNCHANGED <<phase,moved,detached,extracted,held,readFailed,probed,unauthorized,identity>>
DropSource == /\ phase>0 /\ phase'=0 /\ budget'=0 /\ event'=10
              /\ UNCHANGED <<spent,moved,detached,extracted,held,copyTried,copied,copyOk,
                             readFailed,probed,unauthorized,identity>>
DropHeld == /\ held /\ held'=FALSE /\ event'=11
            /\ UNCHANGED <<phase,budget,spent,moved,detached,extracted,copyTried,copied,copyOk,
                           readFailed,probed,unauthorized,identity>>
DropCopy == /\ copied /\ copied'=FALSE /\ event'=12
            /\ UNCHANGED <<phase,budget,spent,moved,detached,extracted,held,copyTried,copyOk,
                           readFailed,probed,unauthorized,identity>>
Probe == /\ ~probed /\ probed'=TRUE /\ unauthorized'=(Fault="inventAuthority") /\ event'=13
         /\ UNCHANGED <<phase,budget,spent,moved,detached,extracted,held,copyTried,copied,copyOk,
                        readFailed,identity>>
Step == Freeze \/ Reader \/ Move \/ Detach \/ Reattach \/ Read \/ FailRead \/ Extract \/ Copy
        \/ DropSource \/ DropHeld \/ DropCopy \/ Probe
Next == /\ Step /\ storageAlive'=(phase'>0) /\ contextAlive'=(phase'>0)
        /\ capAlive'=(phase'>0 \/ copied' \/ (held' /\ ~(Fault="loseHeld" /\ phase'=0)))
Spec == Init /\ [][Next]_vars
LiveSpec == Spec /\ WF_vars(Next /\ event'=10) /\ WF_vars(Next /\ event'=11)
                 /\ WF_vars(Next /\ event'=12)
TypeOK == /\ phase\in 0..4 /\ budget\in 0..Capacity /\ spent\in 0..(2*Capacity+RootCost)
          /\ identity\in {1,2} /\ event\in 0..13
          /\ \A x\in {moved,detached,extracted,held,copyTried,copied,copyOk,readFailed,
                       probed,unauthorized,storageAlive,contextAlive,capAlive}: x\in BOOLEAN
Authority == ~unauthorized /\ identity=1
BudgetConservation == phase\in {3,4} => budget+spent=Capacity
AtomicCopy == copied => copyOk
OwnerLifetime == storageAlive=(phase>0) /\ contextAlive=(phase>0)
CapabilityLifetime == capAlive=(phase>0 \/ copied \/ held)
Reclaims == <> (~storageAlive /\ ~contextAlive /\ ~capAlive)
=============================================================================
