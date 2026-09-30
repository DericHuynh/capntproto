------------------------- MODULE EnumScopeIdentity -------------------------
EXTENDS Naturals, FiniteSets
CONSTANT Fault
\* Compiled C++ enums erase enclosing brands; loader-owned enums retain them.
\* Five handles: default Tone, Text-scope Tone, Data-scope Tone, a Text-list
\* element alias, and a different enum State. Three actions after mode selection
\* operate on an identity-key cache and a Text-scope Tone field. Rust/C++ replay
\* probes every handle and reads the actual field after each transition.
\* No schema parsing, arbitrary brands, union setters or RPC scheduling proof.
VARIABLES mode, started, selected, mask, expectedMask, result, expectedResult,
          value, expectedValue, size, event, steps
vars == <<mode,started,selected,mask,expectedMask,result,expectedResult,
          value,expectedValue,size,event,steps>>
Canonical(i) == IF i=4 THEN 4 ELSE IF mode=0 THEN 0 ELSE IF i=3 THEN 1 ELSE i
Key(i) == CASE Fault="eraseLoadedBrand" /\ mode=1 /\ i<4 -> 0
           [] Fault="retainCompiledBrand" /\ mode=0 /\ i\in {1,2} -> i
           [] Fault="loseListBrand" /\ mode=1 /\ i=3 -> 0
           [] Fault="mergeEnumTypes" /\ i=4 -> Canonical(1)
           [] OTHER -> Canonical(i)
Has(m,i) == (m \div 2^i) % 2
Put(m,i) == m+(1-Has(m,i))*2^i
Count(m) == Cardinality({i\in 0..4: Has(m,i)=1})
Compatible == Canonical(selected)=Canonical(1)
Init == /\ mode=0 /\ started=0 /\ selected=0 /\ mask=0 /\ expectedMask=0
        /\ result=0 /\ expectedResult=0 /\ value=0 /\ expectedValue=0
        /\ size=0 /\ event=0 /\ steps=0
Start(m) == /\ started=0 /\ mode'=m /\ started'=1 /\ event'=10+m
            /\ UNCHANGED <<selected,mask,expectedMask,result,expectedResult,
                           value,expectedValue,size,steps>>
Ready == started=1 /\ steps<3
Select(i) == /\ Ready /\ i#selected /\ selected'=i /\ event'=1+i
             /\ result'=0 /\ expectedResult'=0 /\ steps'=steps+1
             /\ UNCHANGED <<mode,started,mask,expectedMask,value,expectedValue,size>>
Insert == /\ Ready /\ event'=6 /\ steps'=steps+1
          /\ result'=1-Has(mask,Key(selected))
          /\ expectedResult'=1-Has(expectedMask,Canonical(selected))
          /\ mask'=Put(mask,Key(selected))
          /\ expectedMask'=Put(expectedMask,Canonical(selected))
          /\ size'=Count(mask')
          /\ UNCHANGED <<mode,started,selected,value,expectedValue>>
Probe == /\ Ready /\ event'=7 /\ steps'=steps+1
         /\ result'=Has(mask,Key(selected))
         /\ expectedResult'=Has(expectedMask,Canonical(selected))
         /\ UNCHANGED <<mode,started,selected,mask,expectedMask,value,expectedValue,size>>
Write(v,e) == /\ Ready /\ event'=e /\ steps'=steps+1
              /\ result'=IF Compatible \/ (Fault="acceptWrongBrand" /\ mode=1 /\ selected=2) THEN 1 ELSE 0
              /\ expectedResult'=IF Compatible THEN 1 ELSE 0
              /\ value'=IF result'=0 THEN value ELSE IF Fault="truncateUnknown" /\ v=65535 THEN 1 ELSE v
              /\ expectedValue'=IF Compatible THEN v ELSE expectedValue
              /\ UNCHANGED <<mode,started,selected,mask,expectedMask,size>>
Next == Start(0) \/ Start(1) \/ (\E i\in 0..4:Select(i)) \/ Insert \/ Probe \/ Write(1,8) \/ Write(65535,9)
Spec == Init /\ [][Next]_vars
TypeOK == /\ mode\in 0..1 /\ started\in 0..1 /\ selected\in 0..4
          /\ mask\in 0..31 /\ expectedMask\in 0..31 /\ result\in 0..1
          /\ expectedResult\in 0..1 /\ value\in {0,1,65535}
          /\ expectedValue\in {0,1,65535} /\ size\in 0..3 /\ event\in 0..11 /\ steps\in 0..3
CacheIdentity == mask=expectedMask /\ size=Count(expectedMask)
OperationResult == result=expectedResult
StoredOrdinal == value=expectedValue
=============================================================================
