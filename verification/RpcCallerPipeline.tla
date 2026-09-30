----------------------- MODULE RpcCallerPipeline -----------------------
EXTENDS Naturals, TLC
CONSTANTS Used, Broken, Fault
VARIABLES redirected, adopted, returned, rootDropped, capDropped,
          oldConnected, directConnected, outcome, oldFinished, directFinished,
          called, route, readyAtCall, finalAtCall, event
vars == <<redirected,adopted,returned,rootDropped,capDropped,oldConnected,
          directConnected,outcome,oldFinished,directFinished,called,route,
          readyAtCall,finalAtCall,event>>
\* One saved public pipeline reference, with an optional completed call before
\* Init. Route: 0 unused, 1 relay promise, 2 direct promise, 3 direct result,
\* 4 rejected. A call on an unauthenticated reference pins its original route.
Init == /\ redirected=FALSE /\ adopted=FALSE /\ returned=FALSE
        /\ rootDropped=FALSE /\ capDropped=FALSE
        /\ oldConnected=TRUE /\ directConnected=TRUE
        /\ outcome=0 /\ oldFinished=0 /\ directFinished=0
        /\ called=FALSE /\ route=0 /\ readyAtCall=FALSE /\ finalAtCall=FALSE /\ event=0
Pinned == Used \/ (called /\ route=1)
Advance(r,a,v,d,k,o,c,e) ==
    /\ redirected'=r /\ adopted'=a /\ returned'=v /\ rootDropped'=d /\ capDropped'=k
    /\ oldConnected'=o /\ directConnected'=c /\ event'=e
    /\ outcome'=IF outcome#0 THEN outcome ELSE
           IF ~d /\ r /\ a /\ (v \/ ~c)
           THEN IF (~v \/ Broken) /\ Fault#"falseResponse" THEN 2 ELSE 1 ELSE 0
    /\ oldFinished'=IF oldFinished#0 THEN oldFinished ELSE
           IF o /\ (d \/ outcome'#0) /\ (~Pinned \/ k) /\ Fault#"leakOld" THEN 1 ELSE 0
    /\ directFinished'=IF directFinished#0 THEN directFinished ELSE
           IF c /\ r /\ a /\ Fault#"leakDirect" /\
              ((d /\ (k \/ Pinned \/ v)) \/ outcome'=2 \/ (v /\ Broken) \/ Fault="earlyFinish")
           THEN IF Fault="doubleFinish" THEN 2 ELSE 1 ELSE 0
    /\ UNCHANGED <<called,route,readyAtCall,finalAtCall>>
Redirect == /\ ~redirected /\ Advance(TRUE,adopted,returned,rootDropped,capDropped,oldConnected,directConnected,1)
Adopt == /\ ~adopted /\ directConnected /\ Advance(redirected,TRUE,returned,rootDropped,capDropped,oldConnected,directConnected,2)
Return == /\ adopted /\ ~returned /\ directConnected /\ Advance(redirected,adopted,TRUE,rootDropped,capDropped,oldConnected,directConnected,3)
DropRoot == /\ redirected /\ adopted /\ ~rootDropped
            /\ Advance(redirected,adopted,returned,TRUE,capDropped,oldConnected,directConnected,4)
DropCap == /\ ~capDropped /\ Advance(redirected,adopted,returned,rootDropped,TRUE,oldConnected,directConnected,5)
DisconnectOld == /\ redirected /\ adopted /\ oldConnected
                 /\ Advance(redirected,adopted,returned,rootDropped,capDropped,FALSE,directConnected,6)
DisconnectDirect == /\ adopted /\ directConnected
                    /\ Advance(redirected,adopted,returned,rootDropped,capDropped,oldConnected,FALSE,7)
Call == /\ ~called /\ ~capDropped /\ called'=TRUE /\ event'=8
        /\ readyAtCall'=(redirected /\ adopted /\ ~Used /\ directConnected /\ ~(returned /\ Broken))
        /\ finalAtCall'=returned
        /\ route'=IF ((redirected /\ adopted) \/ Fault="eager") /\ (~Used \/ Fault="reorder")
                    THEN IF (~directConnected \/ (returned /\ Broken)) /\ Fault#"stale" THEN 4
                         ELSE IF Fault="late" THEN 1 ELSE IF returned THEN 3 ELSE 2
                    ELSE IF oldConnected THEN 1 ELSE 4
        /\ UNCHANGED <<redirected,adopted,returned,rootDropped,capDropped,
                       oldConnected,directConnected,outcome,oldFinished,directFinished>>
Next == Redirect \/ Adopt \/ Return \/ DropRoot \/ DropCap \/ DisconnectOld \/ DisconnectDirect \/ Call
Spec == Init /\ [][Next]_vars
TypeOK == /\ \A b\in {redirected,adopted,returned,rootDropped,capDropped,oldConnected,directConnected,called,readyAtCall,finalAtCall}: b\in BOOLEAN
          /\ outcome\in 0..2 /\ oldFinished\in 0..1 /\ directFinished\in 0..2 /\ route\in 0..4 /\ event\in 0..8
Authorization == route\in {2,3} => redirected /\ adopted
Ordering == Used => route\notin {2,3}
Migration == readyAtCall => route=IF finalAtCall THEN 3 ELSE 2
DeadRoute == called /\ ~Used /\ route\in {2,3} => readyAtCall
Agreement == outcome=1 => returned /\ ~Broken
OldLifetime == oldConnected => (oldFinished=1 <=> (rootDropped \/ outcome#0) /\ (~Pinned \/ capDropped))
Retention == directConnected /\ redirected /\ adopted /\ ~rootDropped /\ outcome#2 /\ ~(returned /\ Broken) => directFinished=0
Cancellation == directConnected /\ redirected /\ adopted /\ rootDropped /\ capDropped => directFinished=1
SingleFinish == directFinished<=1
LiveSpec == Spec /\ WF_vars(Redirect) /\ WF_vars(Adopt) /\ WF_vars(DropRoot) /\ WF_vars(DropCap)
Reclaims == <>(rootDropped /\ capDropped /\ (~directConnected \/ directFinished=1) /\ (~oldConnected \/ oldFinished=1))
=============================================================================
