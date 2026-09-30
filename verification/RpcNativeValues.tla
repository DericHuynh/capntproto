------------------------- MODULE RpcNativeValues -------------------------
EXTENDS Naturals, TLC
CONSTANTS LoseSnapshotCap, PublishFailedWrite, ChangeSnapshot
VARIABLES src, value, dst, captured, taken, written, readFailed, writeFailed,
          changed, alive, event
vars == <<src,value,dst,captured,taken,written,readFailed,writeFailed,changed,alive,event>>
Init == /\ src=1 /\ value=0 /\ dst=0 /\ captured=0 /\ taken=FALSE
        /\ written=FALSE /\ readFailed=FALSE /\ writeFailed=FALSE
        /\ changed=FALSE /\ alive=TRUE /\ event=0
Snapshot == /\ src>0 /\ ~taken /\ value'=src /\ captured'=src /\ taken'=TRUE /\ event'=1
            /\ UNCHANGED <<src,dst,written,readFailed,writeFailed,changed,alive>>
FailRead == /\ src>0 /\ ~readFailed /\ readFailed'=TRUE /\ event'=2
            /\ UNCHANGED <<src,value,dst,captured,taken,written,writeFailed,changed,alive>>
Change == /\ src=1 /\ ~changed /\ src'=2 /\ changed'=TRUE /\ event'=3
          /\ value'=IF ChangeSnapshot /\ value>0 THEN 2 ELSE value
          /\ UNCHANGED <<dst,captured,taken,written,readFailed,writeFailed,alive>>
Encode == /\ value>0 /\ ~written /\ dst'=value /\ written'=TRUE /\ event'=4
          /\ UNCHANGED <<src,value,captured,taken,readFailed,writeFailed,changed,alive>>
FailWrite == /\ value>0 /\ ~writeFailed /\ ~written /\ writeFailed'=TRUE /\ event'=5
             /\ dst'=IF PublishFailedWrite THEN value ELSE dst
             /\ UNCHANGED <<src,value,captured,taken,written,readFailed,changed,alive>>
DropSource == /\ src>0 /\ src'=0 /\ event'=6
              /\ alive'=(dst>0 \/ (value>0 /\ ~LoseSnapshotCap))
              /\ UNCHANGED <<value,dst,captured,taken,written,readFailed,writeFailed,changed>>
DropValue == /\ value>0 /\ value'=0 /\ event'=7 /\ alive'=(src>0 \/ dst>0)
             /\ UNCHANGED <<src,dst,captured,taken,written,readFailed,writeFailed,changed>>
DropOutput == /\ dst>0 /\ dst'=0 /\ event'=8 /\ alive'=(src>0 \/ value>0)
              /\ UNCHANGED <<src,value,captured,taken,written,readFailed,writeFailed,changed>>
Next == Snapshot \/ FailRead \/ Change \/ Encode \/ FailWrite \/ DropSource \/ DropValue \/ DropOutput
Spec == Init /\ [][Next]_vars
TypeOK == /\ src\in 0..2 /\ value\in 0..2 /\ dst\in 0..2 /\ captured\in 0..2
          /\ taken\in BOOLEAN /\ written\in BOOLEAN /\ readFailed\in BOOLEAN
          /\ writeFailed\in BOOLEAN /\ changed\in BOOLEAN /\ alive\in BOOLEAN /\ event\in 0..8
SnapshotIndependent == value>0 => value=captured
OutputIndependent == dst>0 => dst=captured
AtomicWrite == ~written => dst=0
CapabilityOwnership == alive=(src>0 \/ value>0 \/ dst>0)
=============================================================================
