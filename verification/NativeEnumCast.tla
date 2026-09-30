---------------------------- MODULE NativeEnumCast ----------------------------
EXTENDS Naturals
CONSTANT Fault
\* Individual enum casts check only the nominal ID, unlike aggregate casts.
\* Cases: compiled, unregistered/Text, registered/Data, another loader/Text,
\* extended same-ID schema, wrong enum ID, and a non-enum UInt16 value.
\* Three actions choose an ordinal, cast into an open native enum and store it
\* in a union, or inspect the source enumerant. Rust and pinned C++ replay every
\* explored edge prefix. Closed Rust enums' unknown-value rejection is tested
\* separately: C++ enums and the Rust open enums accept every UInt16.
\* This bounds conversion/lookup behavior, not parsing or full RPC behavior.
VARIABLES started, selected, ordinal, stored, tag, expectedStored, expectedTag,
          result, expectedResult, inspected, expectedInspection, event, steps
vars == <<started,selected,ordinal,stored,tag,expectedStored,expectedTag,
          result,expectedResult,inspected,expectedInspection,event,steps>>
Compatible == selected<5
Allowed == CASE Fault="requireRegistration" /\ selected\in {1,3,4} -> FALSE
             [] Fault="checkBrand" /\ selected=2 -> FALSE
             [] Fault="checkOwner" /\ selected=3 -> FALSE
             [] Fault="acceptWrongId" /\ selected=5 -> TRUE
             [] Fault="numericCoercion" /\ selected=6 -> TRUE
             [] OTHER -> Compatible
Member == ordinal < (IF selected=4 THEN 3 ELSE 2)
Inspection == IF selected=6 THEN 3 ELSE IF Member THEN 1 ELSE 2
Init == /\ started=0 /\ selected=0 /\ ordinal=0 /\ stored=77 /\ tag=0
        /\ expectedStored=77 /\ expectedTag=0 /\ result=0 /\ expectedResult=0
        /\ inspected=0 /\ expectedInspection=0 /\ event=0 /\ steps=0
Setup(i) == /\ started=0 /\ started'=1 /\ selected'=i /\ event'=1+i
            /\ UNCHANGED <<ordinal,stored,tag,expectedStored,expectedTag,result,
                           expectedResult,inspected,expectedInspection,steps>>
Ready == started=1 /\ steps<3
SetOrdinal(v,e) == /\ Ready /\ ordinal#v /\ ordinal'=v /\ event'=e /\ steps'=steps+1
                   /\ UNCHANGED <<started,selected,stored,tag,expectedStored,expectedTag,
                                  result,expectedResult,inspected,expectedInspection>>
Cast == /\ Ready /\ event'=21 /\ steps'=steps+1
        /\ result'=IF Allowed THEN 1 ELSE 2
        /\ expectedResult'=IF Compatible THEN 1 ELSE 2
        /\ stored'=IF Allowed THEN IF Fault="truncateUnknown" /\ ordinal>=2 THEN 0 ELSE ordinal
                    ELSE IF Fault="mutateRejected" THEN ordinal ELSE stored
        /\ tag'=IF Allowed \/ Fault="mutateRejected" THEN 1 ELSE tag
        /\ expectedStored'=IF Compatible THEN ordinal ELSE expectedStored
        /\ expectedTag'=IF Compatible THEN 1 ELSE expectedTag
        /\ UNCHANGED <<started,selected,ordinal,inspected,expectedInspection>>
Inspect == /\ Ready /\ event'=22 /\ steps'=steps+1
           /\ inspected'=CASE Fault="loseExtension" /\ selected=4 /\ ordinal=2 -> 2
                            [] Fault="inventUnknown" /\ selected#6 /\ ordinal=65535 -> 1
                            [] OTHER -> Inspection
           /\ expectedInspection'=Inspection
           /\ UNCHANGED <<started,selected,ordinal,stored,tag,expectedStored,expectedTag,result,expectedResult>>
Next == (\E i\in 0..6:Setup(i)) \/ SetOrdinal(0,11) \/ SetOrdinal(1,12)
        \/ SetOrdinal(2,13) \/ SetOrdinal(65535,14) \/ Cast \/ Inspect
Spec == Init /\ [][Next]_vars
TypeOK == /\ started\in 0..1 /\ selected\in 0..6 /\ ordinal\in {0,1,2,65535}
          /\ stored\in {0,1,2,77,65535} /\ expectedStored\in {0,1,2,77,65535}
          /\ tag\in 0..1 /\ expectedTag\in 0..1 /\ result\in 0..2 /\ expectedResult\in 0..2
          /\ inspected\in 0..3 /\ expectedInspection\in 0..3 /\ event\in 0..22 /\ steps\in 0..3
Compatibility == result=expectedResult
StoredValue == stored=expectedStored /\ tag=expectedTag
SourceMember == inspected=expectedInspection
=============================================================================
