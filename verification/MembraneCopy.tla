------------------------------ MODULE MembraneCopy ------------------------------
EXTENDS Naturals
CONSTANT Fault
\* Copy/adopt/drop/reverse-copy/revoke on one local service and two messages.
\* 0=empty, 1=exported (inbound calls), 2=imported (outbound calls),
\* 3=original unwrapped service, 4=broken capability after revoked reversal.
\* Four actions suffice for copy/adopt/revoke/reverse and copy/adopt/drop/revoke.
\* Rust/C++ replay uses struct, struct-list, AnyPointer and capability-list
\* payloads with duplicate/nested references. Calls settle between actions;
\* arbitrary pending RPC schedules and substitution policies are not modeled.
VARIABLES source, dest, orphan, revoked, expectedSource, expectedDest,
          expectedOrphan, seenSource, seenDest, seenOrphan, live, steps, event
vars == <<source,dest,orphan,revoked,expectedSource,expectedDest,expectedOrphan,
          seenSource,seenDest,seenOrphan,live,steps,event>>
Seen(v,r) == IF v\in {1,2} /\ r=1 THEN 4 ELSE v
Alive(s,d,o,r) == IF s=1 \/ d=3 \/ o=3 \/ (r=0 /\ (d\in {1,2} \/ o\in {1,2})) THEN 1 ELSE 0
Record(s,d,o,r) ==
 /\ seenSource'=3*s
 /\ seenDest'=Seen(d,IF Fault="ignoreRevocation" THEN 0 ELSE r)
 /\ seenOrphan'=Seen(o,IF Fault="ignoreRevocation" THEN 0 ELSE r)
 /\ live'=IF Fault="retainDropped" THEN 1 ELSE Alive(s,d,o,r)
Init == /\ source=1 /\ dest=0 /\ orphan=0 /\ revoked=0
        /\ expectedSource=1 /\ expectedDest=0 /\ expectedOrphan=0
        /\ seenSource=3 /\ seenDest=0 /\ seenOrphan=0 /\ live=1
        /\ steps=0 /\ event=0
Copy(direction) ==
 /\ source=1 /\ orphan=0
 /\ LET s==IF Fault="stealSource" THEN 0 ELSE source
        o==CASE Fault="swapDirection" -> 3-direction
                [] Fault="dropCaps" -> 0 [] OTHER -> direction
    IN /\ source'=s /\ orphan'=o /\ Record(s,dest,o,revoked)
 /\ expectedOrphan'=direction /\ event'=direction
 /\ UNCHANGED <<dest,revoked,expectedSource,expectedDest>>
Adopt == /\ orphan#0 /\ orphan'=0 /\ expectedOrphan'=0
         /\ expectedDest'=expectedOrphan /\ event'=3
         /\ LET d==IF Fault="dropOnAdopt" THEN 0 ELSE orphan
            IN /\ dest'=d /\ Record(source,d,0,revoked)
         /\ UNCHANGED <<source,revoked,expectedSource>>
Back(d,e) == /\ dest=d /\ orphan=0 /\ event'=e
             /\ expectedOrphan'=IF revoked=1 THEN 4 ELSE 3
             /\ LET o==CASE Fault="skipUnwrap" -> 3-d
                           [] Fault="resurrect" -> 3
                           [] OTHER -> IF revoked=1 THEN 4 ELSE 3
                IN /\ orphan'=o /\ Record(source,dest,o,revoked)
             /\ UNCHANGED <<source,dest,revoked,expectedSource,expectedDest>>
DropOrphan == /\ orphan#0 /\ orphan'=0 /\ expectedOrphan'=0 /\ event'=6
              /\ Record(source,dest,0,revoked)
              /\ UNCHANGED <<source,dest,revoked,expectedSource,expectedDest>>
ClearDest == /\ dest#0 /\ dest'=0 /\ expectedDest'=0 /\ event'=7
             /\ Record(source,0,orphan,revoked)
             /\ UNCHANGED <<source,orphan,revoked,expectedSource,expectedOrphan>>
ClearSource == /\ source=1 /\ source'=0 /\ expectedSource'=0 /\ event'=8
               /\ Record(0,dest,orphan,revoked)
               /\ UNCHANGED <<dest,orphan,revoked,expectedDest,expectedOrphan>>
Revoke == /\ revoked=0 /\ revoked'=1 /\ event'=9
          /\ Record(source,dest,orphan,1)
          /\ UNCHANGED <<source,dest,orphan,expectedSource,expectedDest,expectedOrphan>>
Next == /\ steps<4 /\ steps'=steps+1
        /\ (Copy(1) \/ Copy(2) \/ Adopt \/ Back(1,4) \/ Back(2,5)
            \/ DropOrphan \/ ClearDest \/ ClearSource \/ Revoke)
Spec == Init /\ [][Next]_vars
TypeOK == /\ source\in 0..1 /\ expectedSource\in 0..1 /\ dest\in 0..4 /\ orphan\in 0..4
          /\ expectedDest\in 0..4 /\ expectedOrphan\in 0..4 /\ revoked\in 0..1
          /\ seenSource\in {0,3} /\ seenDest\in 0..4 /\ seenOrphan\in 0..4
          /\ live\in 0..1 /\ steps\in 0..4 /\ event\in 0..9
Crossing == seenDest=Seen(expectedDest,revoked) /\ seenOrphan=Seen(expectedOrphan,revoked)
SourceUnchanged == seenSource=3*expectedSource
AuthorityLifetime == live=Alive(expectedSource,expectedDest,expectedOrphan,revoked)
=============================================================================
