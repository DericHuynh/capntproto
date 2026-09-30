---------------------------- MODULE RpcServerHooks ----------------------------
EXTENDS Naturals, TLC
CONSTANTS Fail, Reject, Wire, EarlyRedirect, RecallTarget
VARIABLES a,b,c,shortened,revoked,bstarted,ready
vars == <<a,b,c,shortened,revoked,bstarted,ready>>
\* Calls: 0 absent, 1 queued old, 2 running old, 3 old success, 4 dropped,
\*        5 error, 6 queued replacement, 7 replacement success.
\* Shortening: 0 pending, 1 installed (possibly behind barrier), 2 rejected,
\*             3 canceled by revocation.
Init == /\ a=2 /\ b=1 /\ c=0 /\ shortened=0 /\ revoked=FALSE
        /\ bstarted=FALSE /\ ready=0
Blocked(x,y) == x=2 \/ y\in {1,2}
Resolution(s,x,y) == IF s\in {2,3} THEN 2
                    ELSE IF s=1 /\ ~Blocked(x,y) THEN 1 ELSE 0
ReleaseC(z,x,y,s,r) ==
    IF z=6 /\ (~Blocked(x,y) \/ EarlyRedirect)
      THEN IF r /\ RecallTarget THEN 5 ELSE 7
    ELSE IF z=1 /\ ~Blocked(x,y) THEN
      IF r \/ x=5 \/ y=5 THEN 5 ELSE 3
    ELSE z
Shorten == /\ shortened=0 /\ ~revoked
           /\ shortened'=IF Reject THEN 2 ELSE 1
           /\ ready'=Resolution(shortened',a,b)
           /\ UNCHANGED <<a,b,c,revoked,bstarted>>
Send == /\ c=0 /\ shortened#0
        /\ c'=IF shortened=1 THEN ReleaseC(6,a,b,shortened,revoked)
              ELSE IF revoked \/ (Wire /\ Reject) THEN 5
              ELSE ReleaseC(1,a,b,shortened,revoked)
        /\ UNCHANGED <<a,b,shortened,revoked,bstarted,ready>>
FinishA(cancel) ==
    /\ a=2 /\ a'=IF cancel THEN 4 ELSE IF Fail THEN 5 ELSE 3
    /\ b'=IF b=1 THEN IF ~cancel /\ Fail THEN 5 ELSE 2 ELSE b
    /\ bstarted'=(bstarted \/ b'=2)
    /\ ready'=Resolution(shortened,a',b')
    /\ c'=ReleaseC(c,a',b',shortened,revoked)
    /\ UNCHANGED <<shortened,revoked>>
FinishB(cancel) ==
    /\ b=2 \/ (b=1 /\ cancel)
    /\ b'=IF cancel THEN 4 ELSE IF Fail THEN 5 ELSE 3
    /\ ready'=Resolution(shortened,a,b')
    /\ c'=ReleaseC(c,a,b',shortened,revoked)
    /\ UNCHANGED <<a,shortened,revoked,bstarted>>
Revoke ==
    /\ ~revoked /\ revoked'=TRUE
    /\ a'=IF a=2 THEN 5 ELSE a
    /\ b'=IF b\in {1,2} THEN 5 ELSE b
    /\ shortened'=IF shortened=0 THEN 3 ELSE shortened
    /\ ready'=Resolution(shortened',a',b')
    /\ c'=ReleaseC(c,a',b',shortened',TRUE)
    /\ UNCHANGED bstarted
DropC == /\ c\in {1,6} /\ c'=4
         /\ UNCHANGED <<a,b,shortened,revoked,bstarted,ready>>
Next == Shorten \/ Send \/ FinishA(FALSE) \/ FinishA(TRUE) \/ FinishB(FALSE) \/ FinishB(TRUE) \/ Revoke \/ DropC
Spec == Init /\ [][Next]_vars
TypeOK == /\ a\in 2..5 /\ b\in 1..5 /\ c\in {0,1,3,4,5,6,7}
          /\ shortened\in 0..3 /\ revoked\in BOOLEAN /\ bstarted\in BOOLEAN
          /\ ready\in 0..2
NoOvertake == c=7 => ~Blocked(a,b)
IndependentTarget == (shortened=1 /\ c=5) => FALSE
RevokedWork == revoked => ~Blocked(a,b)
SerialCalls == ~(a=2 /\ b=2)
ResolutionBarrier == ready=1 => ~Blocked(a,b)
=============================================================================
