-------------------------- MODULE RpcAnswerSetup --------------------------
EXTENDS Naturals
CONSTANT Fault
VARIABLES redirected, noticed, returned, dropped, callerExpired, noticeExpired,
          linked, outcome, finished, event
vars == <<redirected,noticed,returned,dropped,callerExpired,noticeExpired,
          linked,outcome,finished,event>>
Init == /\ redirected=0 /\ noticed=0 /\ returned=0 /\ dropped=0
        /\ callerExpired=0 /\ noticeExpired=0 /\ linked=0 /\ outcome=0
        /\ finished=0 /\ event=0
\* One authenticated rendezvous, drained executor between inputs. Timers model
\* setup only. A matched method may run indefinitely without timing out.
Advance(r,n,v,d,ce,ne,e) ==
    /\ redirected'=r /\ noticed'=n /\ returned'=v /\ dropped'=d
    /\ callerExpired'=ce /\ noticeExpired'=ne /\ event'=e
    /\ linked'=IF linked=1 \/ (n=1 /\ (r=1 \/ Fault="unauthenticated")
                  /\ d=0 /\ ((ce=0 /\ ne=0) \/ Fault="resurrect")) THEN 1 ELSE 0
    /\ outcome'=IF outcome#0 THEN outcome ELSE
         IF d=0 /\ ce=1 THEN 2 ELSE IF d=0 /\ linked'=1 /\ v=1 THEN 1 ELSE 0
    /\ finished'=IF finished=1 \/ (n=1 /\ (ce=1 \/ (d=1 /\ r=1)
                   \/ (ne=1 /\ Fault#"leak"))) THEN 1 ELSE 0
Redirect == /\ redirected=0 /\ dropped=0
            /\ Advance(1,noticed,returned,dropped,callerExpired,noticeExpired,1)
Notice == /\ noticed=0
          /\ Advance(redirected,1,returned,dropped,callerExpired,noticeExpired,2)
Return == /\ noticed=1 /\ returned=0
          /\ Advance(redirected,noticed,1,dropped,callerExpired,noticeExpired,3)
CallerExpire == /\ redirected=1 /\ dropped=0 /\ callerExpired=0 /\ outcome=0
                /\ (linked=0 \/ Fault="lateTimer")
                /\ Advance(redirected,noticed,returned,dropped,1,noticeExpired,4)
NoticeExpire == /\ noticed=1 /\ finished=0 /\ linked=0 /\ noticeExpired=0
                /\ Advance(redirected,noticed,returned,dropped,callerExpired,1,5)
Drop == /\ dropped=0
        /\ Advance(redirected,noticed,returned,1,callerExpired,noticeExpired,6)
Next == Redirect \/ Notice \/ Return \/ CallerExpire \/ NoticeExpire \/ Drop
Spec == Init /\ [][Next]_vars
TypeOK == /\ \A b\in {redirected,noticed,returned,dropped,callerExpired,noticeExpired,linked,finished}: b\in 0..1
          /\ outcome\in 0..2 /\ event\in 0..6
Authentication == linked=1 => redirected=1 /\ noticed=1
NoResurrection == linked=1 => callerExpired=0 /\ noticeExpired=0
NoDisclosure == outcome=1 => linked=1 /\ returned=1
Reclaimed == noticed=1 /\ noticeExpired=1 => finished=1
LiveSpec == Spec /\ WF_vars(CallerExpire) /\ WF_vars(NoticeExpire)
SetupSettles == /\ (redirected=1 ~> (linked=1 \/ dropped=1 \/ outcome#0))
                /\ (noticed=1 ~> (linked=1 \/ finished=1))
=============================================================================
