----------------------------- MODULE UnixFacade -----------------------------
EXTENDS Naturals
CONSTANT Fault
\* One descriptor-aware connection, owned or borrowed; independent receive
\* limits 0..2, two FD-bearing capabilities in each direction, two transfers.
\* Five actions per scenario, quiescent IO observations. Transfer invokes all
\* four capabilities even if descriptors were truncated. Escaped FDs are held
\* separately from capabilities and retain authority after RPC disconnect.
VARIABLES opened, mode, slimit, climit, live, rounds, serverSeen, clientSeen,
          calls, retained, drain, fdOpen, probed, event, steps
vars == <<opened,mode,slimit,climit,live,rounds,serverSeen,clientSeen,calls,
          retained,drain,fdOpen,probed,event,steps>>
\* C++ maxFdsPerMessage=0 disables sending as well as receiving descriptors.
ServerValue == IF slimit=0 \/ climit=0 THEN 0 ELSE IF slimit=1 THEN 1 ELSE 21
ClientValue == IF climit=0 \/ slimit=0 THEN 0 ELSE IF climit=1 THEN 3 ELSE 43
Init == /\ opened=0 /\ mode=0 /\ slimit=0 /\ climit=0 /\ live=0 /\ rounds=0
        /\ serverSeen=0 /\ clientSeen=0 /\ calls=0 /\ retained=0 /\ drain=0
        /\ fdOpen=0 /\ probed=0 /\ event=0 /\ steps=0
Open(m,s,c) == /\ opened=0 /\ opened'=1 /\ mode'=m /\ slimit'=s /\ climit'=c
               /\ live'=1 /\ event'=10+9*m+3*s+c
               /\ UNCHANGED <<rounds,serverSeen,clientSeen,calls,retained,probed>>
Transfer == /\ live=1 /\ rounds<2 /\ rounds'=rounds+1 /\ event'=1
            /\ serverSeen'=IF Fault="globalLimit" /\ rounds>0 THEN 0
                            ELSE IF Fault="sendWhileDisabled" /\ climit=0 /\ slimit>0
                            THEN IF slimit=1 THEN 1 ELSE 21 ELSE ServerValue
            /\ clientSeen'=IF Fault="swapDescriptors" /\ climit>0
                           THEN IF climit=1 THEN 4 ELSE 34 ELSE ClientValue
            /\ retained'=clientSeen'
            /\ calls'=calls+(IF Fault="loseCapabilities" THEN 2 ELSE 4)
            /\ UNCHANGED <<opened,mode,slimit,climit,live,probed>>
Drain == /\ opened=1 /\ drain=0 /\ event'=2
         /\ UNCHANGED <<opened,mode,slimit,climit,live,rounds,serverSeen,clientSeen,calls,retained,probed>>
Close(e) == /\ live=1 /\ (e=3 \/ mode=1) /\ live'=0 /\ event'=e
            /\ retained'=IF Fault="revokeEscaped" THEN 0 ELSE retained
            /\ UNCHANGED <<opened,mode,slimit,climit,rounds,serverSeen,clientSeen,calls,probed>>
Probe == /\ opened=1 /\ live=0 /\ rounds>0 /\ probed=0 /\ probed'=1 /\ event'=4
         /\ calls'=calls+(IF Fault="lateCall" THEN 1 ELSE 0)
         /\ UNCHANGED <<opened,mode,slimit,climit,live,rounds,serverSeen,clientSeen,retained>>
Observe ==
 /\ fdOpen'=IF live'=1 THEN 1 ELSE IF opened'=0 THEN 0
             ELSE IF Fault="closeBorrowed" THEN 0 ELSE IF Fault="retainOwned" THEN 1 ELSE mode'
 /\ drain'=IF drain=2 THEN 2 ELSE IF event'=2 \/ drain=1
            THEN IF Fault="earlyDrain" \/ live'=0 \/ (mode'=1 /\ Fault#"countBorrowed") THEN 2 ELSE 1
            ELSE drain
Next == /\ steps<5 /\ steps'=steps+1
        /\ ((\E m\in 0..1,s,c\in 0..2: Open(m,s,c)) \/ Transfer \/ Drain \/ Close(3) \/ Close(6) \/ Probe)
        /\ Observe
Spec == Init /\ [][Next]_vars
TypeOK == /\ opened\in 0..1 /\ mode\in 0..1 /\ slimit\in 0..2 /\ climit\in 0..2
          /\ live\in 0..1 /\ rounds\in 0..2 /\ drain\in 0..2 /\ fdOpen\in 0..1
          /\ probed\in 0..1 /\ steps\in 0..5 /\ calls\in 0..9
DescriptorLimits == rounds>0 => serverSeen=ServerValue /\ clientSeen=ClientValue
CallableCapabilities == calls=4*rounds
Ownership == fdOpen=(IF opened=0 THEN 0 ELSE IF live=1 THEN 1 ELSE mode)
DrainContract == /\ (event=2 => drain=(IF live=1 /\ mode=0 THEN 1 ELSE 2))
                 /\ (drain=1 => live=1 /\ mode=0)
EscapedAuthority == rounds>0 => retained=ClientValue
=============================================================================
