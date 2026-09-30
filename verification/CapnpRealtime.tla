-------------------------- MODULE CapnpRealtime --------------------------
EXTENDS Integers, Sequences, FiniteSets, TLC
\* Proposed latest-snapshot capability contract; see REALTIME.md. No new RPC
\* message tags. Data is lossy/unordered; control and receipts are reliable.
CONSTANTS MessageCount, KeyCount, Capacity, SubmitBefore, Lifetime, ClockSkew,
          AllowLoss, AllowDuplicate, AllowCancel, AllowClose, Bug
ASSUME /\ MessageCount \in Nat \ {0} /\ KeyCount \in 1..MessageCount
       /\ Capacity \in 1..KeyCount /\ SubmitBefore \in Nat \ {0}
       /\ Lifetime \in Nat \ {0} /\ ClockSkew \in Nat
       /\ AllowLoss \in BOOLEAN /\ AllowDuplicate \in BOOLEAN
       /\ AllowCancel \in BOOLEAN /\ AllowClose \in BOOLEAN
       /\ Bug \in {"none","admissionOnly","unsafeClock","staleApply",
                    "duplicateApply","overflow","forgetCancel","closeLeak","timeoutClaimsDrop"}
Ids == 1..MessageCount
Keys == 1..KeyCount
Key(n) == ((n-1) % KeyCount)+1
Copies == IF AllowDuplicate THEN {0,1} ELSE {0}
Terminal == {"applied","expired","superseded","busy","canceled","closed"}
Horizon == SubmitBefore+Lifetime+2*ClockSkew
VARIABLE rt
vars == <<rt>>
LocalTime(s) == s.now+s.offset
Timely(s,n) == LocalTime(s)+(IF Bug="unsafeClock" THEN 0 ELSE ClockSkew) < s.rxDeadline[n]
Init == /\ \E offset \in (-ClockSkew)..ClockSkew :
             rt=[now |-> 0, offset |-> offset, submitted |-> {},
                 deadline |-> [n \in Ids |-> 0], rxDeadline |-> [n \in Ids |-> 0], data |-> {}, pending |-> {},
                 high |-> [k \in Keys |-> 0], result |-> [n \in Ids |-> "unseen"],
                 observed |-> [n \in Ids |-> "unsent"], receipts |-> {},
                 cancelWire |-> {}, cancelSent |-> {}, canceled |-> {},
                 senderClosed |-> FALSE, closeRequest |-> FALSE, serverClosed |-> FALSE,
                 closeReply |-> FALSE, closeObserved |-> FALSE, applied |-> <<>>]
\* Finish is receiver-local. A final outcome is recorded before its receipt
\* travels to the sender, so a sender timeout cannot infer non-execution.
Finish(s,ns,outcome) ==
    [s EXCEPT !.pending=@ \ ns,
       !.result=[n \in Ids |-> IF n \in ns THEN outcome ELSE s.result[n]],
       !.receipts=@ \cup {<<n,outcome>> : n \in ns}]
Submit ==
    /\ ~rt.senderClosed /\ rt.now<SubmitBefore /\ Cardinality(rt.submitted)<MessageCount
    /\ LET n==Cardinality(rt.submitted)+1
       IN rt'=[rt EXCEPT !.submitted=@ \cup {n}, !.deadline[n]=rt.now+Lifetime,
                        !.observed[n]="pending",
                        !.data=@ \cup {[seq |-> n,key |-> Key(n),deadline |-> rt.now+Lifetime,copy |-> c] : c \in Copies}]
Receive(packet) ==
    /\ packet \in rt.data
    /\ LET n==packet.seq k==packet.key
           s==[rt EXCEPT !.data=@ \ {packet}, !.rxDeadline[n]=packet.deadline]
           remember==s.result[n] # "unseen" /\ Bug # "duplicateApply" /\
                     ~(Bug="forgetCancel" /\ s.result[n]="canceled")
       IN rt'=
          IF remember THEN
              IF s.result[n] \in Terminal THEN Finish(s,{n},s.result[n]) ELSE s
          ELSE IF s.serverClosed THEN Finish(s,{n},"closed")
          ELSE IF n<s.high[k] /\ Bug # "staleApply" THEN Finish(s,{n},"superseded")
          ELSE LET older=={m \in s.pending : Key(m)=k /\ m # n}
                   replaced==Finish(s,older,"superseded")
                   advanced==[replaced EXCEPT !.high[k]=n]
               IN IF ~Timely(advanced,n) THEN Finish(advanced,{n},"expired")
                  ELSE IF Cardinality(advanced.pending)>=Capacity /\ Bug # "overflow"
                       THEN Finish(advanced,{n},"busy")
                       ELSE [advanced EXCEPT !.pending=@ \cup {n}, !.result[n]="queued"]
Drop(packet) == /\ AllowLoss /\ packet \in rt.data
                /\ rt'=[rt EXCEPT !.data=@ \ {packet}]
Apply(n) ==
    /\ n \in rt.pending /\ n=rt.high[Key(n)]
    /\ (~rt.serverClosed \/ Bug="closeLeak")
    /\ (Timely(rt,n) \/ Bug="admissionOnly")
    /\ rt'=[Finish(rt,{n},"applied") EXCEPT
             !.applied=Append(@,[seq |-> n,key |-> Key(n),at |-> rt.now,
                                deadline |-> rt.rxDeadline[n],closed |-> rt.serverClosed])]
Expire(n) == /\ n \in rt.pending /\ ~Timely(rt,n)
             /\ rt'=Finish(rt,{n},"expired")
SendCancel(n) == /\ AllowCancel /\ ~rt.senderClosed /\ n \in rt.submitted \ rt.cancelSent
                 /\ rt'=[rt EXCEPT !.cancelSent=@ \cup {n}, !.cancelWire=@ \cup {n}]
ReceiveCancel(n) ==
    /\ n \in rt.cancelWire
    /\ LET s==[rt EXCEPT !.cancelWire=@ \ {n}]
       IN rt'=IF s.result[n] \in Terminal THEN Finish(s,{n},s.result[n])
              ELSE IF s.serverClosed THEN Finish(s,{n},"closed")
              ELSE [Finish(s,{n},"canceled") EXCEPT !.canceled=@ \cup {n}]
SendClose == /\ AllowClose /\ ~rt.senderClosed
             /\ rt'=[rt EXCEPT !.senderClosed=TRUE, !.closeRequest=TRUE]
ReceiveClose ==
    /\ rt.closeRequest
    /\ LET s==IF Bug="closeLeak" THEN rt ELSE Finish(rt,rt.pending,"closed")
       IN rt'=[s EXCEPT !.serverClosed=TRUE, !.closeRequest=FALSE, !.closeReply=TRUE]
ObserveClose == /\ rt.closeReply
                /\ rt'=[rt EXCEPT !.closeReply=FALSE, !.closeObserved=TRUE]
Observe(reply) == /\ reply \in rt.receipts
                 /\ rt'=[rt EXCEPT !.receipts=@ \ {reply}, !.observed[reply[1]]=reply[2]]
Timeout(n) == /\ rt.observed[n]="pending" /\ rt.now>=rt.deadline[n]
              /\ rt'=[rt EXCEPT !.observed[n]=IF Bug="timeoutClaimsDrop" THEN "expired" ELSE "unknown"]
Tick == /\ rt.now<Horizon /\ rt'=[rt EXCEPT !.now=@+1]
ReceiveAny == \E p \in rt.data : Receive(p)
ApplyAny == \E n \in Ids : Apply(n)
ExpireAny == \E n \in Ids : Expire(n)
CancelAny == \E n \in Ids : ReceiveCancel(n)
ObserveAny == \E r \in rt.receipts : Observe(r)
Next == Submit \/ ReceiveAny \/ (\E p \in rt.data : Drop(p)) \/ ApplyAny \/ ExpireAny
        \/ (\E n \in Ids : SendCancel(n) \/ Timeout(n)) \/ CancelAny
        \/ SendClose \/ ReceiveClose \/ ObserveClose \/ ObserveAny \/ Tick
Spec == Init /\ [][Next]_vars
LiveSpec == Spec /\ WF_vars(Tick) /\ WF_vars(ReceiveAny) /\ WF_vars(ApplyAny)
            /\ WF_vars(ExpireAny) /\ WF_vars(CancelAny) /\ WF_vars(ObserveAny)
            /\ WF_vars(ReceiveClose) /\ WF_vars(ObserveClose)
            /\ (\A n \in Ids : WF_vars(Timeout(n)))
TypeOK ==
    /\ rt.now \in 0..Horizon /\ rt.offset \in (-ClockSkew)..ClockSkew
    /\ rt.submitted \subseteq Ids /\ rt.pending \subseteq rt.submitted
    /\ rt.deadline \in [Ids -> 0..(SubmitBefore+Lifetime)]
    /\ rt.rxDeadline \in [Ids -> 0..(SubmitBefore+Lifetime)]
    /\ rt.high \in [Keys -> 0..MessageCount]
    /\ rt.data \subseteq [seq:rt.submitted,key:Keys,deadline:1..(SubmitBefore+Lifetime),copy:Copies]
    /\ rt.result \in [Ids -> Terminal \cup {"unseen","queued"}]
    /\ rt.observed \in [Ids -> Terminal \cup {"unsent","pending","unknown"}]
    /\ rt.receipts \subseteq rt.submitted \X Terminal
    /\ rt.cancelWire \subseteq rt.submitted /\ rt.cancelSent \subseteq rt.submitted
    /\ rt.canceled \subseteq rt.submitted
    /\ rt.senderClosed \in BOOLEAN /\ rt.serverClosed \in BOOLEAN
    /\ rt.closeRequest \in BOOLEAN /\ rt.closeReply \in BOOLEAN /\ rt.closeObserved \in BOOLEAN
    /\ rt.applied \in Seq([seq:Ids,key:Keys,at:0..Horizon,
                          deadline:0..(SubmitBefore+Lifetime),closed:BOOLEAN])
PacketIntegrity == /\ \A p \in rt.data : p.key=Key(p.seq) /\ p.deadline=rt.deadline[p.seq]
                   /\ \A n \in Ids : rt.rxDeadline[n] # 0 => rt.rxDeadline[n]=rt.deadline[n]
NoLateApplication == \A i \in 1..Len(rt.applied) : rt.applied[i].at<rt.applied[i].deadline
AtMostOnce == \A i,j \in 1..Len(rt.applied) : rt.applied[i].seq=rt.applied[j].seq => i=j
NoRollback == \A i,j \in 1..Len(rt.applied) :
    (i<j /\ rt.applied[i].key=rt.applied[j].key) => rt.applied[i].seq<rt.applied[j].seq
QueueBound == Cardinality(rt.pending)<=Capacity
OnePendingPerKey == \A n,m \in rt.pending : Key(n)=Key(m) => n=m
PendingFreshness == \A n \in rt.pending : rt.result[n]="queued" /\ n=rt.high[Key(n)]
ReceiptSoundness == \A n \in rt.submitted : rt.observed[n] \in Terminal => rt.observed[n]=rt.result[n]
ReceiptsConsistent == \A a,b \in rt.receipts : a[1]=b[1] => a[2]=b[2]
NeverApplyCanceled == \A i \in 1..Len(rt.applied) : rt.applied[i].seq \notin rt.canceled
CloseBarrier == /\ \A i \in 1..Len(rt.applied) : ~rt.applied[i].closed
                /\ rt.closeObserved => rt.serverClosed
AppliedOutcome == \A n \in rt.submitted :
    rt.result[n]="applied" <=> (\E i \in 1..Len(rt.applied) : rt.applied[i].seq=n)
AllOffersDecide == \A n \in Ids : n \in rt.submitted ~> rt.observed[n] \notin {"pending","unsent"}
CloseCompletes == rt.senderClosed ~> rt.closeObserved
EventuallyDrained == <>[](rt.now=Horizon /\ rt.pending={} /\ rt.data={} /\ rt.receipts={} /\
    rt.cancelWire={} /\ ~rt.closeRequest /\ ~rt.closeReply)
NoAppliedWitness == rt.applied= <<>>
NoExpiredWitness == ~\E n \in Ids : rt.result[n]="expired"
NoSupersededWitness == ~\E n \in Ids : rt.result[n]="superseded"
NoBusyWitness == ~\E n \in Ids : rt.result[n]="busy"
NoCanceledWitness == rt.canceled={}
NoUnknownWitness == ~\E n \in Ids : rt.observed[n]="unknown"
NoLateReceiptWitness == ~\E n \in Ids : rt.observed[n]="unknown" /\ rt.result[n]="applied"
NoClosedWitness == ~rt.closeObserved
NoCancelAfterApplyWitness == ~\E n \in rt.cancelSent : rt.observed[n]="applied" /\ n \notin rt.cancelWire
NoLostWitness == ~\E n \in rt.submitted : rt.observed[n]="unknown" /\ rt.result[n]="unseen" /\
    ~\E p \in rt.data : p.seq=n
=============================================================================
