-------------------- MODULE StorageHistory --------------------
EXTENDS Naturals, TLC
CONSTANT Fault
\* Three already-staged entries. Publishing may skip drafts. One history-
\* preserving checkpoint, one trimming checkpoint and one restart, in any order.
VARIABLES published, e1, e2, e3, a1, a2, a3, r1, r2, r3, floor,
          committedFloor, preserved, trimmed, restarted, q0, q1, q2, q3, q4
core == <<published,e1,e2,e3,a1,a2,a3,r1,r2,r3,floor,committedFloor,
          preserved,trimmed,restarted>>
queries == <<q0,q1,q2,q3,q4>>
vars == <<core,queries>>
Set(b1,b2,b3) == {r \in 1..3: CASE r=1 -> b1 [] r=2 -> b2 [] OTHER -> b3}
Events == Set(e1,e2,e3)
Acknowledged == Set(a1,a2,a3)
Entries == Set(r1,r2,r3)
First(s) == CHOOSE r \in s: \A x \in s: r<=x
Last(s) == CHOOSE r \in s: \A x \in s: r>=x
\* 0=wait, 1..3=event, 4=invalid cursor, 5=explicit history gap.
Query(c) == IF c<floor THEN 5
            ELSE IF c>published \/ (c#0 /\ c\notin Events) THEN 4
            ELSE LET available == {r \in (IF Fault="draft" THEN Entries ELSE Events): r>c}
                 IN IF available={} THEN 0
                    ELSE IF Fault="skip" THEN Last(available) ELSE First(available)
View == /\ q0=Query(0) /\ q1=Query(1) /\ q2=Query(2)
        /\ q3=Query(3) /\ q4=Query(4)
Init == /\ published=0 /\ e1=FALSE /\ e2=FALSE /\ e3=FALSE
        /\ a1=FALSE /\ a2=FALSE /\ a3=FALSE
        /\ r1=TRUE /\ r2=TRUE /\ r3=TRUE /\ floor=0 /\ committedFloor=0
        /\ preserved=FALSE /\ trimmed=FALSE /\ restarted=FALSE /\ View
Publish(r) == /\ r\in Entries /\ r>published /\ published'=r
              /\ e1'=(e1 \/ r=1) /\ e2'=(e2 \/ r=2) /\ e3'=(e3 \/ r=3)
              /\ a1'=(a1 \/ r=1) /\ a2'=(a2 \/ r=2) /\ a3'=(a3 \/ r=3)
              /\ UNCHANGED <<r1,r2,r3,floor,committedFloor,preserved,trimmed,restarted>>
Preserve == /\ ~preserved /\ preserved'=TRUE
            /\ r1'=(r1 /\ (e1 \/ 1>published))
            /\ r2'=(r2 /\ (e2 \/ 2>published)) /\ r3'=r3
            /\ e1'=(e1 /\ Fault#"loseEvent")
            /\ UNCHANGED <<published,e2,e3,a1,a2,a3,floor,committedFloor,trimmed,restarted>>
Trim == /\ ~trimmed /\ trimmed'=TRUE
        /\ floor'=published /\ committedFloor'=published
        /\ e1'=(published=1) /\ e2'=(published=2) /\ e3'=(published=3)
        /\ r1'=(r1 /\ 1>=published) /\ r2'=(r2 /\ 2>=published) /\ r3'=r3
        /\ UNCHANGED <<published,a1,a2,a3,preserved,restarted>>
Restart == /\ ~restarted /\ restarted'=TRUE
           /\ floor'=(IF Fault="forgetFloor" THEN 0 ELSE floor)
           /\ UNCHANGED <<published,e1,e2,e3,a1,a2,a3,r1,r2,r3,committedFloor,preserved,trimmed>>
Next == /\ ((\E r\in 1..3: Publish(r)) \/ Preserve \/ Trim \/ Restart)
        /\ q0'=Query(0)' /\ q1'=Query(1)' /\ q2'=Query(2)'
        /\ q3'=Query(3)' /\ q4'=Query(4)'
Spec == Init /\ [][Next]_vars
TypeOK == /\ published\in 0..3 /\ floor\in 0..3 /\ committedFloor\in 0..3
          /\ \A b\in {e1,e2,e3,a1,a2,a3,r1,r2,r3,preserved,trimmed,restarted}: b\in BOOLEAN
          /\ \A q\in {q0,q1,q2,q3,q4}: q\in 0..5
RetainedEvents == Events={r\in Acknowledged: r>=committedFloor}
PayloadRetained == Events\subseteq Entries /\ 3\in Entries
FloorPreserved == floor=committedFloor
PublicationOnly == \A c\in 0..4: Query(c)\in 1..3 => Query(c)\in Acknowledged
EarliestEvent == \A c\in 0..4: Query(c)\in 1..3 =>
                   /\ Query(c)>c
                   /\ \A r\in Events: r>c => Query(c)<=r
ExplicitGap == \A c\in 0..4: (Query(c)=5)=(c<committedFloor)
=============================================================================
