------------------------ MODULE RpcOrphanConcat ------------------------
EXTENDS Naturals, TLC
CONSTANT Fault
VARIABLES sourceA, sourceB, location, shrunk, changedSource, changedCopy,
          read, held, failed, badType, sourceValue, copyValue, snapshot,
          unknown, address, refsA, refsB
vars == <<sourceA,sourceB,location,shrunk,changedSource,changedCopy,read,held,
          failed,badType,sourceValue,copyValue,snapshot,unknown,address,refsA,refsB>>
Owner == location \in {1,2}
Bit(b) == IF b THEN 1 ELSE 0
Init == /\ sourceA=TRUE /\ sourceB=TRUE /\ location=0 /\ shrunk=FALSE /\ changedSource=FALSE /\ changedCopy=FALSE
        /\ read=FALSE /\ held=FALSE /\ failed=FALSE /\ badType=FALSE /\ sourceValue=1 /\ copyValue=0 /\ snapshot=0
        /\ unknown=TRUE /\ address=0 /\ refsA=1 /\ refsB=1
Concat == /\ location=0 /\ sourceA /\ sourceB /\ location'=1
          /\ snapshot'=sourceValue /\ copyValue'=sourceValue /\ address'=1
          /\ unknown'=(Fault#"loseUnknown")
          /\ refsA'=refsA+(IF Fault="loseCopyCap" THEN 0 ELSE 1) /\ refsB'=refsB+1
          /\ UNCHANGED <<sourceA,sourceB,shrunk,changedSource,changedCopy,read,held,failed,badType,sourceValue>>
FailCopy == /\ location=0 /\ sourceA /\ sourceB /\ ~failed /\ failed'=TRUE
            /\ refsA'=refsA+(IF Fault="leakFailure" THEN 1 ELSE 0)
            /\ UNCHANGED <<sourceA,sourceB,location,shrunk,changedSource,changedCopy,read,held,badType,
                            sourceValue,copyValue,snapshot,unknown,address,refsB>>
BadType == /\ location=0 /\ sourceA /\ ~badType /\ badType'=TRUE
           /\ unknown'=(Fault#"ignoreType")
           /\ UNCHANGED <<sourceA,sourceB,location,shrunk,changedSource,changedCopy,read,held,failed,
                           sourceValue,copyValue,snapshot,address,refsA,refsB>>
EditSource == /\ sourceA /\ ~changedSource /\ changedSource'=TRUE /\ sourceValue'=2
              /\ UNCHANGED <<sourceA,sourceB,location,shrunk,changedCopy,read,held,failed,badType,
                              copyValue,snapshot,unknown,address,refsA,refsB>>
EditCopy == /\ location=1 /\ ~changedCopy /\ changedCopy'=TRUE /\ copyValue'=3
            /\ sourceValue'=(IF Fault="shallowCopy" THEN 3 ELSE sourceValue)
            /\ UNCHANGED <<sourceA,sourceB,location,shrunk,changedSource,read,held,failed,badType,
                            snapshot,unknown,address,refsA,refsB>>
Shrink == /\ location=1 /\ ~shrunk /\ shrunk'=TRUE
          /\ refsB'=refsB-(IF Fault="leakTruncated" THEN 0 ELSE 1)
          /\ address'=(IF Fault="moveOnShrink" THEN 2 ELSE address)
          /\ UNCHANGED <<sourceA,sourceB,location,changedSource,changedCopy,read,held,failed,badType,
                          sourceValue,copyValue,snapshot,unknown,refsA>>
Read == /\ Owner /\ ~read /\ read'=TRUE /\ held'=TRUE
        /\ refsA'=refsA+(IF Fault="loseHeld" THEN 0 ELSE 1)
        /\ UNCHANGED <<sourceA,sourceB,location,shrunk,changedSource,changedCopy,failed,badType,
                        sourceValue,copyValue,snapshot,unknown,address,refsB>>
Release == /\ held /\ held'=FALSE /\ refsA'=refsA-1
           /\ UNCHANGED <<sourceA,sourceB,location,shrunk,changedSource,changedCopy,read,failed,badType,
                           sourceValue,copyValue,snapshot,unknown,address,refsB>>
DropA == /\ sourceA /\ sourceA'=FALSE /\ refsA'=refsA-1
         /\ UNCHANGED <<sourceB,location,shrunk,changedSource,changedCopy,read,held,failed,badType,
                         sourceValue,copyValue,snapshot,unknown,address,refsB>>
DropB == /\ sourceB /\ sourceB'=FALSE /\ refsB'=refsB-1
         /\ UNCHANGED <<sourceA,location,shrunk,changedSource,changedCopy,read,held,failed,badType,
                         sourceValue,copyValue,snapshot,unknown,address,refsA>>
Adopt == /\ location=1 /\ location'=2
         /\ UNCHANGED <<sourceA,sourceB,shrunk,changedSource,changedCopy,read,held,failed,badType,
                         sourceValue,copyValue,snapshot,unknown,address,refsA,refsB>>
Drop == /\ Owner /\ location'=3 /\ refsA'=refsA-1 /\ refsB'=refsB-Bit(~shrunk)
        /\ UNCHANGED <<sourceA,sourceB,shrunk,changedSource,changedCopy,read,held,failed,badType,
                        sourceValue,copyValue,snapshot,unknown,address>>
Next == Concat \/ FailCopy \/ BadType \/ EditSource \/ EditCopy \/ Shrink \/ Read \/ Release \/ DropA \/ DropB \/ Adopt \/ Drop
Spec == Init /\ [][Next]_vars
TypeOK == /\ location\in 0..3 /\ sourceValue\in 1..3 /\ copyValue\in 0..3 /\ snapshot\in 0..2
          /\ address\in 0..2 /\ refsA\in 0..4 /\ refsB\in 0..2
          /\ \A b\in {sourceA,sourceB,shrunk,changedSource,changedCopy,read,held,failed,badType,unknown}:b\in BOOLEAN
Ownership == /\ refsA=Bit(sourceA)+Bit(Owner)+Bit(held)
             /\ refsB=Bit(sourceB)+Bit(Owner /\ ~shrunk)
Independence == /\ sourceValue=(IF changedSource THEN 2 ELSE 1)
                /\ (location#0 => copyValue=(IF changedCopy THEN 3 ELSE snapshot))
Preserved == unknown
StableShrink == (location#0 => address=1)
=============================================================================
