----------------------- MODULE SchemaMemberIdentity -----------------------
EXTENDS Naturals, FiniteSets
CONSTANT Fault
\* Nine public handles: field lookup aliases 1/2, a second field 3, another
\* brand 4, another metadata owner 5, declaring/inherited method aliases 6/7,
\* and enum ordinals 8/9. Three cache operations, with an initially selected
\* handle, suffice to insert, select an alias and probe/remove it. Native replay
\* also probes every handle after each operation. Keys are immutable snapshots.
\* This finite cache model does not model metadata parsing, numeric hash values,
\* arbitrary schema graphs, concurrent loading, or the RPC wire protocol.
VARIABLES selected, mask, expectedMask, result, expectedResult, size, event, steps
vars == <<selected,mask,expectedMask,result,expectedResult,size,event,steps>>
Canonical(i) == IF i=2 THEN 1 ELSE IF i=7 THEN 6 ELSE i
Key(i) == CASE Fault="dropOwner" /\ i=5 -> 1
           [] Fault="dropBrand" /\ i=4 -> 1
           [] Fault="dropIndex" /\ i=3 -> 1
           [] Fault="wrongInheritedOwner" /\ i=7 -> 7
           [] Fault="splitAlias" /\ i=2 -> 2
           [] OTHER -> Canonical(i)
Bit(i) == 2^(i-1)
Has(m,i) == (m \div Bit(i)) % 2
Put(m,i) == m + (1-Has(m,i))*Bit(i)
Remove(m,i) == m - Has(m,i)*Bit(i)
Count(m) == Cardinality({i\in 1..9: Has(m,i)=1})
Init == /\ selected=1 /\ mask=0 /\ expectedMask=0 /\ result=0
        /\ expectedResult=0 /\ size=0 /\ event=0 /\ steps=0
Select(i) == /\ steps<3 /\ i#selected /\ selected'=i /\ event'=i
             /\ result'=0 /\ expectedResult'=0 /\ steps'=steps+1
             /\ UNCHANGED <<mask,expectedMask,size>>
Insert == /\ steps<3 /\ event'=10 /\ steps'=steps+1
          /\ result'=1-Has(mask,Key(selected))
          /\ expectedResult'=1-Has(expectedMask,Canonical(selected))
          /\ mask'=Put(mask,Key(selected))
          /\ expectedMask'=Put(expectedMask,Canonical(selected))
          /\ size'=Count(mask') /\ UNCHANGED selected
Probe == /\ steps<3 /\ event'=11 /\ steps'=steps+1
         /\ result'=Has(mask,Key(selected))
         /\ expectedResult'=Has(expectedMask,Canonical(selected))
         /\ UNCHANGED <<selected,mask,expectedMask,size>>
Erase == /\ steps<3 /\ event'=12 /\ steps'=steps+1
         /\ result'=Has(mask,Key(selected))
         /\ expectedResult'=Has(expectedMask,Canonical(selected))
         /\ mask'=Remove(mask,Key(selected))
         /\ expectedMask'=Remove(expectedMask,Canonical(selected))
         /\ size'=Count(mask') /\ UNCHANGED selected
Next == (\E i\in 1..9: Select(i)) \/ Insert \/ Probe \/ Erase
Spec == Init /\ [][Next]_vars
TypeOK == /\ selected\in 1..9 /\ mask\in 0..511 /\ expectedMask\in 0..511
          /\ result\in 0..1 /\ expectedResult\in 0..1 /\ size\in 0..3
          /\ event\in 0..12 /\ steps\in 0..3
CacheIdentity == /\ mask=expectedMask /\ result=expectedResult
                 /\ size=Count(expectedMask)
=============================================================================
