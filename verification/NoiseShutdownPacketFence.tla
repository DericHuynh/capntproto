---------------------- MODULE NoiseShutdownPacketFence ----------------------
EXTENDS Naturals
CONSTANTS Fault, Crossed, Valid, ReplyFits, ReplyIntact
\* One locally drained request exists. Its peer supplies a two-part receipt;
\* crossed requests and two input bytes can arrive in either order. ReplyFits
\* bounds the peer's fixed stream credit (no credit replenishment in this model).
\* ReplyIntact=FALSE resets the reciprocal send stream immediately before close.
\* Closing: 0=open, 1=authenticated graceful code, 2=other application/transport
\* close. Outcome: 0=pending, 1=receipt, 2=failure. External failure is sticky.
VARIABLES fragment, receipt, peer, data, reply, closing, failed, outcome, event
vars == <<fragment,receipt,peer,data,reply,closing,failed,outcome,event>>
Init == /\ fragment=0 /\ receipt=0 /\ peer=0 /\ data=0 /\ reply=0
        /\ closing=0 /\ failed=0 /\ outcome=0 /\ event=0
Open == closing=0 /\ failed=0
Prefix == /\ Open /\ fragment=0 /\ fragment'=1
          /\ receipt'=(IF Fault="skipFin" THEN 1 ELSE receipt) /\ event'=1
          /\ UNCHANGED <<peer,data,reply,closing,failed,outcome>>
Finish == /\ Open /\ fragment=1 /\ fragment'=2 /\ event'=2
          /\ receipt'=(IF Valid \/ Fault="binding" THEN 1 ELSE 0)
          /\ failed'=(IF Valid \/ Fault="binding" THEN 0 ELSE 1)
          /\ outcome'=(IF Valid \/ Fault="binding" THEN 0 ELSE 2)
          /\ UNCHANGED <<peer,data,reply,closing>>
Request == /\ Open /\ Crossed /\ peer=0 /\ peer'=1
           /\ reply'=(IF data=2 /\ ReplyFits THEN 1 ELSE 0) /\ event'=3
           /\ UNCHANGED <<fragment,receipt,data,closing,failed,outcome>>
Deliver == /\ Open /\ Crossed /\ data<2 /\ data'=data+1
           /\ reply'=(IF peer=1 /\ data'=2 /\ ReplyFits THEN 1 ELSE 0) /\ event'=4
           /\ UNCHANGED <<fragment,receipt,peer,closing,failed,outcome>>
Close(kind) ==
    /\ closing=0 /\ closing'=kind /\ event'=4+kind
    /\ LET success == receipt=1 /\ (kind=1 \/ Fault="anyClose")
                      /\ (~Crossed \/ Fault="crossed" \/
                          (peer=1 /\ data=2 /\ (reply=1 \/ Fault="reply")
                           /\ (ReplyIntact \/ Fault="reset")))
       IN outcome'=(IF outcome#0 /\ Fault#"resurrect" THEN outcome
                    ELSE IF success THEN 1 ELSE 2)
    /\ UNCHANGED <<fragment,receipt,peer,data,reply,failed>>
Fail == /\ Open /\ failed'=1 /\ outcome'=2 /\ event'=7
        /\ UNCHANGED <<fragment,receipt,peer,data,reply,closing>>
HealthyNext == Prefix \/ Finish \/ Request \/ Deliver \/ Close(1)
Next == HealthyNext \/ Close(2) \/ Fail
Spec == Init /\ [][Next]_vars
LiveSpec == Init /\ [][HealthyNext]_vars /\ WF_vars(HealthyNext)
\* A cooperative peer closes only after both legs have completed. This check
\* applies to Valid receipts with sufficient credit to queue the whole reply.
SuccessNext == Prefix \/ Finish \/ Request \/ Deliver \/
               (Close(1) /\ receipt=1 /\ (~Crossed \/ reply=1))
SuccessSpec == Init /\ [][SuccessNext]_vars /\ WF_vars(SuccessNext)
TypeOK == /\ fragment\in 0..2 /\ receipt\in 0..1 /\ peer\in 0..1
          /\ data\in 0..2 /\ reply\in 0..1 /\ closing\in 0..2
          /\ failed\in 0..1 /\ outcome\in 0..2 /\ event\in 0..7
CompleteFrame == receipt=1 => fragment=2
Binding == receipt=1 => Valid
GracefulClose == outcome=1 => closing=1
CrossedFence == outcome=1 /\ Crossed => peer=1 /\ data=2
ReplyFence == outcome=1 /\ Crossed => reply=1
ResetFence == outcome=1 /\ Crossed => ReplyIntact
Terminal == failed=1 => outcome=2
ReceiptFence == outcome=1 => receipt=1
\* This establishes termination, including a premature-close error. Success
\* additionally requires the peer to finish its exchange before closing.
Progress == <>(outcome#0)
Success == <>(outcome=1)
=============================================================================
