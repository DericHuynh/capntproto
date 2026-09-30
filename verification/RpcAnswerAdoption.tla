------------------------ MODULE RpcAnswerAdoption ------------------------
EXTENDS Naturals, TLC
CONSTANTS Broken, Fault
VARIABLES redirected, adopted, returned, dropped, connected, outcome,
          oldFinished, directFinished, capHeld, event
vars == <<redirected,adopted,returned,dropped,connected,outcome,oldFinished,directFinished,capHeld,event>>
Init == /\ redirected=FALSE /\ adopted=FALSE /\ returned=FALSE /\ dropped=FALSE
        /\ connected=TRUE /\ outcome=0 /\ oldFinished=0 /\ directFinished=0 /\ capHeld=FALSE /\ event=0
\* Atomic input followed by drained Rust executor work. The response may arrive
\* before authorization. A received response survives later link failure, while
\* an outstanding response fails. Success retains the direct question until drop.
Advance(r,a,v,d,c,e) ==
    /\ redirected'=r /\ adopted'=a /\ returned'=v /\ dropped'=d /\ connected'=c /\ event'=e
    /\ outcome'=(IF outcome#0 THEN outcome ELSE
          IF ~d /\ a /\ (r \/ Fault="eager") /\ (v \/ ~c)
          THEN IF (Broken /\ Fault#"falseSuccess") \/ ~v THEN 2 ELSE 1 ELSE 0)
    /\ oldFinished'=(IF d \/ outcome'#0 THEN 1 ELSE oldFinished)
    /\ directFinished'=(IF directFinished#0 THEN directFinished ELSE
          IF c /\ a /\ r /\ ((d /\ Fault#"leakCancel") \/ outcome'=2 \/ (outcome'=1 /\ Fault="earlyFinish"))
          THEN IF Fault="doubleFinish" THEN 2 ELSE 1 ELSE 0)
    /\ capHeld'=(outcome'=1 /\ ~d)
Redirect == /\ ~redirected /\ Advance(TRUE,adopted,returned,dropped,connected,1)
Adopt == /\ connected /\ ~adopted /\ Advance(redirected,TRUE,returned,dropped,connected,2)
Return == /\ connected /\ adopted /\ ~returned /\ Advance(redirected,adopted,TRUE,dropped,connected,3)
Drop == /\ ~dropped /\ Advance(redirected,adopted,returned,TRUE,connected,4)
Disconnect == /\ connected /\ adopted /\ Advance(redirected,adopted,returned,dropped,FALSE,5)
Next == Redirect \/ Adopt \/ Return \/ Drop \/ Disconnect
Spec == Init /\ [][Next]_vars
TypeOK == /\ \A b\in {redirected,adopted,returned,dropped,connected,capHeld}:b\in BOOLEAN
          /\ outcome\in 0..2 /\ oldFinished\in 0..1 /\ directFinished\in 0..2 /\ event\in 0..5
Authorization == outcome#0 => redirected /\ adopted
Agreement == outcome=1 => returned /\ ~Broken
RetainedAuthority == capHeld => directFinished=0
Cancellation == connected /\ redirected /\ adopted /\ dropped => directFinished=1
SingleFinish == directFinished<=1
OldLifetime == oldFinished=1 <=> dropped \/ outcome#0
LiveSpec == Spec /\ WF_vars(Redirect) /\ WF_vars(Adopt) /\ WF_vars(Return) /\ WF_vars(Drop)
Reclaims == <>(dropped /\ (~connected \/ directFinished=1))
=============================================================================
