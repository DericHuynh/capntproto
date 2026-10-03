--------------------------- MODULE NativeIdentity ---------------------------
EXTENDS Naturals
CONSTANT Fault
\* Two RFC key pairs, a third invalid public label, and four API operations per
\* history. Keys are abstract identities, not scalar bytes or cryptography.
\* Config owns a copy; dropping Identity must not revoke that configuration.
\* No entropy, memory-erasure, handshake-security or arbitrary-key proof here.
VARIABLES key, label, configured, configuredLabel, ownedConfig,
          authenticated, result, expectedResult, event, steps
vars == <<key,label,configured,configuredLabel,ownedConfig,
          authenticated,result,expectedResult,event,steps>>
Init == /\ key=0 /\ label=0 /\ configured=0 /\ configuredLabel=0
        /\ ownedConfig=0 /\ authenticated=0 /\ result=0 /\ expectedResult=0
        /\ event=0 /\ steps=0
Import(s,p) ==
    /\ steps<4 /\ key=0
    /\ LET allow == IF Fault="acceptMismatch" THEN TRUE
                    ELSE IF Fault="rejectValid" THEN FALSE ELSE s=p
       IN /\ key'=(IF allow THEN s ELSE 0) /\ label'=(IF allow THEN p ELSE 0)
          /\ result'=(IF allow THEN 1 ELSE 0)
    /\ expectedResult'=(IF s=p THEN 1 ELSE 0)
    /\ event'=(s-1)*3+p /\ steps'=steps+1
    /\ UNCHANGED <<configured,configuredLabel,ownedConfig,authenticated>>
Derive(s) ==
    /\ steps<4 /\ key=0 /\ key'=s
    /\ label'=(IF Fault="wrongDerived" THEN 3-s ELSE s)
    /\ result'=1 /\ expectedResult'=1 /\ event'=s+6 /\ steps'=steps+1
    /\ UNCHANGED <<configured,configuredLabel,ownedConfig,authenticated>>
Configure ==
    /\ steps<4 /\ key#0 /\ configured'=key /\ ownedConfig'=key
    /\ configuredLabel'=(IF Fault="mislabelConfig" THEN 3-label ELSE label)
    /\ authenticated'=0 /\ result'=1 /\ expectedResult'=1
    /\ event'=9 /\ steps'=steps+1 /\ UNCHANGED <<key,label>>
DropIdentity ==
    /\ steps<4 /\ key#0 /\ key'=0 /\ label'=0
    /\ configured'=(IF Fault="forgetConfig" THEN 0 ELSE configured)
    /\ configuredLabel'=(IF Fault="forgetConfig" THEN 0 ELSE configuredLabel)
    /\ result'=1 /\ expectedResult'=1 /\ event'=10 /\ steps'=steps+1
    /\ UNCHANGED <<ownedConfig,authenticated>>
Authenticate ==
    /\ steps<4 /\ configured#0 /\ authenticated=0
    /\ authenticated'=configuredLabel /\ result'=1 /\ expectedResult'=1
    /\ event'=11 /\ steps'=steps+1
    /\ UNCHANGED <<key,label,configured,configuredLabel,ownedConfig>>
DropConfig ==
    /\ steps<4 /\ configured#0 /\ configured'=0 /\ configuredLabel'=0
    /\ ownedConfig'=0 /\ authenticated'=0 /\ result'=1 /\ expectedResult'=1
    /\ event'=12 /\ steps'=steps+1 /\ UNCHANGED <<key,label>>
Next == (\E s\in 1..2,p\in 1..3: Import(s,p)) \/ (\E s\in 1..2: Derive(s))
        \/ Configure \/ DropIdentity \/ Authenticate \/ DropConfig
Spec == Init /\ [][Next]_vars
TypeOK == /\ key\in 0..2 /\ label\in 0..3 /\ configured\in 0..2
          /\ configuredLabel\in 0..3 /\ ownedConfig\in 0..2
          /\ authenticated\in 0..3 /\ result\in 0..1 /\ expectedResult\in 0..1
          /\ event\in 0..12 /\ steps\in 0..4
LiveBinding == key=label
ConfigBinding == configured=configuredLabel
ConfigOwnership == configured=ownedConfig
AuthBinding == authenticated=0 \/ authenticated=configured
ExpectedOutcome == result=expectedResult
=============================================================================
