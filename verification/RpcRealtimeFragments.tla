----------------------- MODULE RpcRealtimeFragments -----------------------
EXTENDS Naturals, TLC
CONSTANT Bug
VARIABLES binding, mask, bad, status, verified, applied, otherBinding, other,
          otherCanceled, closed, expired, unauthorized, event
vars == <<binding,mask,bad,status,verified,applied,otherBinding,other,
          otherCanceled,closed,expired,unauthorized,event>>
\* Two fragments of sequence 1 and one competing partial sequence 2. The
\* reassembly quota is one; incomplete values are Unknown and never supersede
\* an admitted snapshot. Bit masks retain exact positions, not packet counts.
Init == /\ binding=0 /\ mask=0 /\ bad=FALSE /\ status=0 /\ verified=FALSE
        /\ applied=0 /\ otherBinding=FALSE /\ other=FALSE /\ otherCanceled=FALSE
        /\ closed=FALSE /\ expired=FALSE /\ unauthorized=FALSE /\ event=0
Has(m,b) == IF b=1 THEN m\in {1,3} ELSE m\in {2,3}
Add(m,b) == IF Has(m,b) THEN m ELSE m+b
Fragment(b,corrupt,wrong,e) ==
    LET allowed == ~closed /\ (status=0 \/ (Bug="replay" /\ status=2))
                   /\ (~wrong \/ Bug="authority")
        bind == IF allowed /\ binding=0 THEN 1 ELSE binding
        accepted == allowed /\ bind=1 /\ ~expired /\ (~other \/ Bug="quota")
        next == IF accepted THEN Add(mask,b) ELSE mask
        damaged == bad \/ (accepted /\ ~Has(mask,b) /\ corrupt)
        complete == accepted /\ next=3
        valid == complete /\ ~damaged
        admit == complete /\ (~damaged \/ Bug="hash")
    IN /\ binding'=bind /\ mask'=IF complete THEN 0 ELSE next
       /\ bad'=IF complete THEN FALSE ELSE damaged
       /\ status'=IF admit THEN 1 ELSE status
       /\ verified'=IF admit THEN valid ELSE verified
       /\ unauthorized'=(unauthorized \/ (wrong /\ accepted))
       /\ event'=e
       /\ UNCHANGED <<applied,otherBinding,other,otherCanceled,closed,expired>>
First == Fragment(1,FALSE,FALSE,1)
Second == Fragment(2,FALSE,FALSE,2)
Corrupt == Fragment(1,TRUE,FALSE,3)
WrongToken == Fragment(1,FALSE,TRUE,4)
Conflict == /\ binding#0 /\ binding'=IF Bug="metadata" THEN 2 ELSE binding
            /\ event'=5
            /\ UNCHANGED <<mask,bad,status,verified,applied,otherBinding,other,
                           otherCanceled,closed,expired,unauthorized>>
Malformed == /\ event'=6 /\ UNCHANGED <<binding,mask,bad,status,verified,applied,
                          otherBinding,other,otherCanceled,closed,expired,unauthorized>>
Cancel == /\ status\in {0,1} /\ status'=IF closed THEN 5 ELSE 3
          /\ mask'=0 /\ bad'=FALSE /\ event'=7
          /\ UNCHANGED <<binding,verified,applied,otherBinding,other,otherCanceled,
                         closed,expired,unauthorized>>
Apply == /\ status=1 /\ status'=2 /\ applied'=applied+1 /\ event'=8
         /\ UNCHANGED <<binding,mask,bad,verified,otherBinding,other,otherCanceled,
                        closed,expired,unauthorized>>
Close(e) == /\ ~closed /\ closed'=TRUE /\ event'=e
            /\ mask'=IF Bug="close" THEN mask ELSE 0
            /\ bad'=IF Bug="close" THEN bad ELSE FALSE
            /\ other'=IF Bug="close" THEN other ELSE FALSE
            /\ status'=IF status=1 THEN 5 ELSE status
            /\ UNCHANGED <<binding,verified,applied,otherBinding,otherCanceled,
                           expired,unauthorized>>
Tick == /\ ~expired /\ expired'=TRUE /\ event'=10
        /\ mask'=IF Bug="expiry" THEN mask ELSE 0
        /\ bad'=IF Bug="expiry" THEN bad ELSE FALSE
        /\ other'=IF Bug="expiry" THEN other ELSE FALSE
        /\ status'=IF status=1 THEN 4 ELSE status
        /\ UNCHANGED <<binding,verified,applied,otherBinding,otherCanceled,closed,unauthorized>>
Compete == /\ event'=11
           /\ otherBinding'=(otherBinding \/ (~closed /\ ~otherCanceled))
           /\ other'=(other \/ (~closed /\ ~otherCanceled /\ ~expired /\
                                (mask=0 \/ Bug="quota")))
           /\ UNCHANGED <<binding,mask,bad,status,verified,applied,otherCanceled,
                          closed,expired,unauthorized>>
CancelOther == /\ ~closed /\ ~otherCanceled /\ otherCanceled'=TRUE /\ other'=FALSE
               /\ event'=12
               /\ UNCHANGED <<binding,mask,bad,status,verified,applied,otherBinding,
                              closed,expired,unauthorized>>
Next == First \/ Second \/ Corrupt \/ WrongToken \/ Conflict \/ Malformed
        \/ Cancel \/ Apply \/ Close(9) \/ Tick \/ Compete \/ CancelOther \/ Close(13)
Spec == Init /\ [][Next]_vars
TypeOK == /\ binding\in 0..2 /\ mask\in 0..3 /\ status\in 0..5 /\ applied\in 0..2
          /\ \A b\in {bad,verified,otherBinding,other,otherCanceled,closed,expired,unauthorized}: b\in BOOLEAN
          /\ event\in 0..13
Integrity == status\in {1,2,4} => verified
AtMostOnce == applied<=1
Quota == other => mask=0
Authority == ~unauthorized
Immutable == binding<=1
Lifetime == closed => mask=0 /\ ~other /\ status#1
Deadline == expired => mask=0 /\ ~other /\ status#1
NoPartialReceipt == mask#0 => status=0
=============================================================================
