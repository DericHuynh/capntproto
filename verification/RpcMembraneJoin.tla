-------------------------- MODULE RpcMembraneJoin --------------------------
EXTENDS Naturals, TLC
CONSTANTS Allowed, Mixed, Equal, Fault
VARIABLES started, begun, reply0, reply1, revoked, dropped, connected, outcome,
          finished, checks, used, protected, event
vars == <<started,begun,reply0,reply1,revoked,dropped,connected,outcome,
          finished,checks,used,protected,event>>
\* A complete batch must be authorized before unwrapping. Two underlying wire
\* parts may return in either order, even after cancellation or revocation.
Init == /\ started=FALSE /\ begun=FALSE /\ reply0=FALSE /\ reply1=FALSE
        /\ revoked=FALSE /\ dropped=FALSE /\ connected=TRUE /\ outcome=0
        /\ finished=0 /\ checks=0 /\ used=FALSE /\ protected=TRUE /\ event=0
Advance(s,b,r0,r1,v,d,c,e) ==
    /\ started'=s /\ begun'=b /\ reply0'=r0 /\ reply1'=r1
    /\ revoked'=v /\ dropped'=d /\ connected'=c /\ event'=e
    /\ checks'=IF ~started /\ s /\ ~v /\ ~Mixed THEN 1 ELSE checks
    /\ outcome'=IF outcome#0 THEN outcome ELSE IF s /\ ~d THEN
                    IF (v /\ Fault#"revoke") \/ ~c \/ ~b THEN 3 ELSE
                    IF b /\ r0 /\ (r1 \/ Fault="early")
                       THEN IF Equal \/ Fault="false" THEN 1 ELSE 2 ELSE 0
                 ELSE 0
    /\ finished'=IF finished#0 THEN finished ELSE
                   IF c /\ b /\ (d \/ outcome'#0) /\ Fault#"leak"
                     THEN IF Fault="double" THEN 4 ELSE 2 ELSE 0
    /\ protected'=IF outcome'=1 THEN Fault#"unwrap" ELSE protected
    /\ UNCHANGED used
Start == /\ ~started
         /\ Advance(TRUE,~revoked /\ (Allowed \/ Fault="authorize") /\
                    (~Mixed \/ Fault="mixed"),reply0,reply1,revoked,dropped,connected,1)
Reply0 == /\ begun /\ connected /\ ~reply0
          /\ Advance(started,begun,TRUE,reply1,revoked,dropped,connected,2)
Reply1 == /\ begun /\ connected /\ ~reply1
          /\ Advance(started,begun,reply0,TRUE,revoked,dropped,connected,3)
Revoke == /\ ~revoked
          /\ Advance(started,begun,reply0,reply1,TRUE,dropped,connected,4)
Drop == /\ started /\ ~dropped
        /\ Advance(started,begun,reply0,reply1,revoked,TRUE,connected,5)
Disconnect == /\ begun /\ connected
              /\ Advance(started,begun,reply0,reply1,revoked,dropped,FALSE,6)
Use == /\ outcome=1 /\ ~dropped /\ ~used /\ used'=TRUE /\ event'=7
       /\ UNCHANGED <<started,begun,reply0,reply1,revoked,dropped,connected,outcome,
                      finished,checks,protected>>
Next == Start \/ Reply0 \/ Reply1 \/ Revoke \/ Drop \/ Disconnect \/ Use
Spec == Init /\ [][Next]_vars
TypeOK == /\ \A b\in {started,begun,reply0,reply1,revoked,dropped,connected,used,protected}: b\in BOOLEAN
          /\ outcome\in 0..3 /\ finished\in {0,2,4} /\ checks\in 0..1 /\ event\in 0..7
Authorization == begun => Allowed /\ ~Mixed
Equality == outcome=1 => Equal
CompleteBatch == outcome\in {1,2} => reply0 /\ reply1
PolicyBoundary == outcome=1 => protected
Revocation == started /\ ~dropped /\ revoked => outcome#0
Retention == begun /\ connected /\ (dropped \/ outcome#0) => finished=2
SingleFinish == finished<=2
=============================================================================
