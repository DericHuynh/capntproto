---------------------- MODULE RpcGenericParameters ----------------------
EXTENDS Naturals, FiniteSets, Sequences, TLC
CONSTANT Fault
VARIABLES group,a,b,c,seen,used,encounter,emitted,alias
vars == <<group,a,b,c,seen,used,encounter,emitted,alias>>
Has(mask,bit) == (mask \div bit) % 2 = 1
Bit(i) == CASE i=1 -> 1 [] i=2 -> 2 [] i=3 -> 4
Add(mask,i) == IF i=0 THEN mask ELSE IF Has(mask,Bit(i)) THEN mask ELSE mask+Bit(i)
Ordered(mask) == (IF Has(mask,1) THEN 100 ELSE 0)+(IF Has(mask,2) THEN 20 ELSE 0)+(IF Has(mask,4) THEN 3 ELSE 0)
\* Order is encoded using fixed declaration slots. Encounter order uses packed
\* digits; Canonical packs the lexical order for the same subset.
Canonical(mask) == CASE mask=0 -> 0 [] mask=1 -> 1 [] mask=2 -> 2 [] mask=3 -> 12
                       [] mask=4 -> 3 [] mask=5 -> 13 [] mask=6 -> 23 [] mask=7 -> 123
Init == /\ group\in BOOLEAN /\ a\in 0..3 /\ b\in 0..3 /\ c\in 0..3
        /\ seen=0 /\ used=0 /\ encounter=0 /\ emitted=FALSE /\ alias=0
Visit == \E i\in 1..3:
    /\ ~Has(seen,Bit(i)) /\ seen'=seen+Bit(i)
    /\ LET p == <<a,b,c>>[i] IN
       /\ used'=Add(used,p)
       /\ encounter'=IF used'=used THEN encounter ELSE encounter*10+p
    /\ UNCHANGED <<group,a,b,c,emitted,alias>>
Emit == /\ seen=7 /\ ~emitted /\ emitted'=TRUE
        /\ alias'=CASE Fault="encounterOrder" -> encounter
                       [] Fault="omitOuter" -> Canonical(IF Has(used,1) THEN used-1 ELSE used)
                       [] Fault="omitUnusedGroup" -> Canonical(used)
                       [] OTHER -> Canonical(IF group THEN 7 ELSE used)
        /\ UNCHANGED <<group,a,b,c,seen,used,encounter>>
Next == Visit \/ Emit
Spec == Init /\ [][Next]_vars
TypeOK == /\ group\in BOOLEAN /\ a\in 0..3 /\ b\in 0..3 /\ c\in 0..3 /\ seen\in 0..7 /\ used\in 0..7
          /\ encounter\in Nat /\ alias\in Nat /\ emitted\in BOOLEAN
AliasOrder == emitted => alias=Canonical(IF group THEN 7 ELSE Add(Add(Add(0,a),b),c))
=============================================================================
