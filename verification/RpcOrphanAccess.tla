------------------------ MODULE RpcOrphanAccess ------------------------
EXTENDS Naturals, TLC
\* Edge-prefix histories run against both compiled and runtime-loaded schemas.
\* Schema registry/brand mismatches refine type rejection; concrete metadata
\* and lifetimes are checked by Rust regressions and compiler acceptance cases.
CONSTANT Fault
VARIABLES location, size, first, second, held, edited, read, shrunk, grown,
          typed, typeError, arenaError, typeSafe, address, alive1, alive2, alive3, ambient
vars == <<location,size,first,second,held,edited,read,shrunk,grown,typed,typeError,arenaError,
          typeSafe,address,alive1,alive2,alive3,ambient>>
Init == /\ location=0 /\ size=0 /\ first=0 /\ second=0 /\ held=0
        /\ edited=FALSE /\ read=FALSE /\ shrunk=FALSE /\ grown=FALSE
        /\ typed=FALSE /\ typeError=FALSE /\ arenaError=FALSE /\ typeSafe=TRUE
        /\ address=0 /\ alive1=FALSE /\ alive2=FALSE /\ alive3=FALSE /\ ambient=TRUE
Allocate == /\ location=0 /\ location'=1 /\ size'=2 /\ first'=1 /\ second'=2 /\ address'=1
            /\ UNCHANGED <<held,edited,read,shrunk,grown,typed,typeError,arenaError,typeSafe>>
Edit == /\ location=1 /\ ~edited /\ edited'=TRUE /\ first'=3
        /\ UNCHANGED <<location,size,second,held,read,shrunk,grown,typed,typeError,arenaError,typeSafe,address>>
Read == /\ location\in {1,2} /\ ~read /\ read'=TRUE /\ held'=first
        /\ UNCHANGED <<location,size,first,second,edited,shrunk,grown,typed,typeError,arenaError,typeSafe,address>>
Shrink == /\ location=1 /\ ~shrunk /\ shrunk'=TRUE /\ size'=1
          /\ second'=(IF Fault="retainRemoved" THEN second ELSE 0)
          /\ address'=(IF Fault="copyChildren" THEN 2 ELSE address)
          /\ UNCHANGED <<location,first,held,edited,read,grown,typed,typeError,arenaError,typeSafe>>
Grow == /\ location=1 /\ shrunk /\ ~grown /\ grown'=TRUE /\ size'=3
        /\ address'=(IF Fault="copyChildren" THEN 2 ELSE address)
        /\ UNCHANGED <<location,first,second,held,edited,read,shrunk,typed,typeError,arenaError,typeSafe>>
Typed == /\ location=1 /\ ~typed /\ typed'=TRUE
         /\ UNCHANGED <<location,size,first,second,held,edited,read,shrunk,grown,typeError,arenaError,typeSafe,address>>
BadType == /\ location=1 /\ ~typeError /\ typeError'=TRUE
           /\ typeSafe'=(Fault#"ignoreType")
           /\ UNCHANGED <<location,size,first,second,held,edited,read,shrunk,grown,typed,arenaError,address>>
BadArena == /\ location=1 /\ ~arenaError /\ arenaError'=TRUE
            /\ UNCHANGED <<location,size,first,second,held,edited,read,shrunk,grown,typed,typeError,typeSafe,address>>
Adopt == /\ location=1 /\ location'=2
         /\ UNCHANGED <<size,first,second,held,edited,read,shrunk,grown,typed,typeError,arenaError,typeSafe,address>>
Drop == /\ location\in {1,2} /\ location'=3 /\ first'=0 /\ second'=0
        /\ UNCHANGED <<size,held,edited,read,shrunk,grown,typed,typeError,arenaError,typeSafe,address>>
Release == /\ held#0 /\ held'=0
           /\ UNCHANGED <<location,size,first,second,edited,read,shrunk,grown,typed,typeError,arenaError,typeSafe,address>>
Next == /\ (Allocate \/ Edit \/ Read \/ Shrink \/ Grow \/ Typed \/ BadType \/ BadArena \/ Adopt \/ Drop \/ Release)
        /\ alive1'=(IF Fault="leakOld" /\ edited' THEN TRUE ELSE first'=1 \/ (held'=1 /\ Fault#"loseHeld"))
        /\ alive2'=(second'=2)
        /\ alive3'=(Fault#"loseEdited" /\ (first'=3 \/ held'=3))
        /\ ambient'=~(Fault="stealAmbient" /\ edited')
Spec == Init /\ [][Next]_vars
TypeOK == /\ location\in 0..3 /\ size\in 0..3 /\ first\in {0,1,3} /\ second\in {0,2}
          /\ held\in {0,1,3} /\ address\in 0..2
          /\ \A b\in {edited,read,shrunk,grown,typed,typeError,arenaError,typeSafe,alive1,alive2,alive3,ambient}:b\in BOOLEAN
Ownership == /\ alive1=(first=1 \/ held=1) /\ alive2=(second=2) /\ alive3=(first=3 \/ held=3)
Isolation == ambient
NoTruncationLeak == (size=1 => second=0)
TypePreserved == typeSafe
ShallowMovement == (location#0 => address=1)
=============================================================================
