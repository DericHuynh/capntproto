--------------------------- MODULE RpcReconnect ---------------------------
EXTENDS Naturals, TLC
CONSTANTS DisconnectError, ResetControl, CaptureAtSend, IgnoreControlGeneration
VARIABLES first, second, epoch, firstEpoch, secondEpoch, changed, target, connects, probed, observed
vars == <<first,second,epoch,firstEpoch,secondEpoch,changed,target,connects,probed,observed>>
\* Two requests are constructed on generation zero. Sending and consuming their
\* errors can straddle explicit replacement/reset and automatic reconnection.
\* Target: 0 original, 1 reconnect callback, 2 explicit replacement, 3 lazy reset.
Init == /\ first = 0 /\ second = 0 /\ epoch = 0 /\ firstEpoch = 0 /\ secondEpoch = 0
        /\ changed = FALSE /\ target = 0 /\ connects = 0 /\ probed = FALSE /\ observed = 3
SendFirst == /\ first = 0 /\ first' = 1
             /\ firstEpoch' = IF CaptureAtSend THEN epoch ELSE 0
             /\ UNCHANGED <<second,epoch,secondEpoch,changed,target,connects,probed,observed>>
SendSecond == /\ second = 0 /\ second' = 1
              /\ secondEpoch' = IF CaptureAtSend THEN epoch ELSE 0
              /\ UNCHANGED <<first,epoch,firstEpoch,changed,target,connects,probed,observed>>
Reconnect(e) == /\ epoch' = IF DisconnectError /\ e = epoch THEN epoch + 1 ELSE epoch
                /\ connects' = IF DisconnectError /\ e = epoch THEN connects + 1 ELSE connects
                /\ target' = IF DisconnectError /\ e = epoch THEN 1 ELSE target
FinishFirst == /\ first = 1 /\ first' = 2 /\ Reconnect(firstEpoch)
               /\ UNCHANGED <<second,firstEpoch,secondEpoch,changed,probed,observed>>
FinishSecond == /\ second = 1 /\ second' = 2 /\ Reconnect(secondEpoch)
                /\ UNCHANGED <<first,firstEpoch,secondEpoch,changed,probed,observed>>
Change == /\ ~changed /\ changed' = TRUE
          /\ epoch' = IF IgnoreControlGeneration THEN epoch ELSE epoch + 1
          /\ target' = IF ResetControl THEN 3 ELSE 2
          /\ UNCHANGED <<first,second,firstEpoch,secondEpoch,connects,probed,observed>>
Probe == /\ first = 2 /\ second = 2 /\ changed /\ ~probed /\ probed' = TRUE
         /\ target' = IF target = 3 THEN 1 ELSE target
         /\ connects' = IF target = 3 THEN connects + 1 ELSE connects
         /\ observed' = target'
         /\ UNCHANGED <<first,second,epoch,firstEpoch,secondEpoch,changed>>
Next == SendFirst \/ SendSecond \/ FinishFirst \/ FinishSecond \/ Change \/ Probe
Spec == Init /\ [][Next]_vars
TypeOK == /\ first \in 0..2 /\ second \in 0..2 /\ epoch \in 0..3
          /\ firstEpoch \in 0..3 /\ secondEpoch \in 0..3
          /\ changed \in BOOLEAN /\ probed \in BOOLEAN /\ target \in 0..3
          /\ connects \in 0..3 /\ observed \in 0..3
ReplacementProtected == changed => target = (IF ResetControl THEN (IF probed THEN 1 ELSE 3) ELSE 2)
OneReconnectPerGeneration == connects <= (IF ResetControl /\ probed THEN 2 ELSE 1)
ProbeProvenance == probed => observed = (IF ResetControl THEN 1 ELSE 2)
=============================================================================
