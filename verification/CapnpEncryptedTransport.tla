--------------------- MODULE CapnpEncryptedTransport ---------------------
EXTENDS Naturals, Sequences, FiniteSets, TLC
\* Proposed VatNetwork contract with ideal authenticated encryption, NOT a
\* concrete Noise handshake or a proof about cryptographic primitives.
\* Recipient 1, trusted introducer 2, host 3, active network attacker 4.
\* The introducer distributes an epoch-specific PSK over protected existing
\* channels. It is trusted with that key. Capability scope is "service" only.
CONSTANTS EpochCount, MessageCount, Attack, Bug
ASSUME /\ EpochCount \in Nat \ {0} /\ MessageCount \in Nat \ {0}
       /\ Attack \in {"none","forge","tamper","replay","capability","epoch"}
       /\ Bug \in {"none","skipAuthentication","unboundCapability","staleSession",
                       "replay","reuseNonce","cleartext"}
Epochs == 1..EpochCount
Messages == 1..MessageCount
Principals == 1..4
Frame(e,n,key,body) == [epoch |-> e, nonce |-> n, key |-> key,
    caller |-> 1, host |-> 3, cap |-> "service", body |-> body, clear |-> FALSE]
Packet(f,cap,session,copy) == [frame |-> f, useCap |-> cap, session |-> session, copy |-> copy]
VARIABLES current, issued, knows, sent, sealed, wire, handled, accepted,
          seen, confirmed, confirmWire, exposed, attacked
vars == <<current,issued,knows,sent,sealed,wire,handled,accepted,seen,confirmed,confirmWire,exposed,attacked>>
Init == /\ current=1 /\ issued={} /\ knows=[p \in Principals |-> IF p=4 THEN {0} ELSE {}]
        /\ sent={} /\ sealed={} /\ wire={} /\ handled={} /\ accepted= <<>>
        /\ seen={} /\ confirmed={} /\ confirmWire={} /\ exposed={} /\ attacked=FALSE
Issue == /\ current \notin issued /\ issued'=issued \cup {current}
         /\ knows'=[knows EXCEPT ![2]=@ \cup {current}]
         /\ UNCHANGED <<current,sent,sealed,wire,handled,accepted,seen,confirmed,confirmWire,exposed,attacked>>
Credential(p,e) ==
    /\ p \in {1,3} /\ e \in issued \ knows[p]
    /\ knows'=[knows EXCEPT ![p]=@ \cup {e}]
    /\ UNCHANGED <<current,issued,sent,sealed,wire,handled,accepted,seen,confirmed,confirmWire,exposed,attacked>>
Emit(n) ==
    /\ current \in knows[1] /\ n \in Messages /\ <<current,n>> \notin sent
    /\ LET nonce==IF Bug="reuseNonce" THEN 1 ELSE n
           f==[Frame(current,nonce,current,n) EXCEPT !.clear=(Bug="cleartext")]
       IN /\ sealed'=sealed \cup {f}
          /\ wire'=wire \cup {Packet(f,"service",current,0)}
    /\ sent'=sent \cup {<<current,n>>}
    /\ UNCHANGED <<current,issued,knows,handled,accepted,seen,confirmed,confirmWire,exposed,attacked>>
\* The attacker can construct a valid ciphertext only with a known key.
\* Changing a protected field of an observed ciphertext fails authentication.
Authentic(f) == f \in sealed \/ f.key \in knows[4]
CanAccept(p) ==
    LET f==p.frame
    IN /\ (Bug="skipAuthentication" \/ (Authentic(f) /\ f.key \in knows[3] /\ f.key=f.epoch))
       /\ f.caller=1 /\ f.host=3 /\ f.cap="service"
       /\ (Bug="unboundCapability" \/ p.useCap=f.cap)
       /\ (Bug="staleSession" \/ (f.epoch=current /\ p.session=current))
       /\ (Bug="replay" \/ <<f.key,f.nonce>> \notin seen)
Receive(p) ==
    /\ p \in wire
    \* Honest zero-RTT data waits for the independent host-side introduction.
    /\ p.frame.key=0 \/ p.frame.key \in knows[3]
    /\ wire'=wire \ {p} /\ handled'=handled \cup {p}
    /\ IF CanAccept(p)
       THEN /\ accepted'=Append(accepted,[frame |-> p.frame,useCap |-> p.useCap,atEpoch |-> current])
            /\ seen'=seen \cup {<<p.frame.key,p.frame.nonce>>}
            /\ confirmWire'=confirmWire \cup {p.frame.epoch}
       ELSE UNCHANGED <<accepted,seen,confirmWire>>
    /\ UNCHANGED <<current,issued,knows,sent,sealed,confirmed,exposed,attacked>>
Inject ==
    /\ ~attacked /\ Attack # "none"
    /\ LET candidates ==
           IF Attack="forge" THEN {Packet(Frame(current,1,0,1),"service",current,1)}
           ELSE {CASE Attack="tamper" -> Packet([f EXCEPT !.body=MessageCount+1],"service",f.epoch,1)
                 [] Attack="capability" -> Packet(f,"admin",f.epoch,1)
                 [] Attack="epoch" -> Packet(f,"service",current,1)
                 [] OTHER -> Packet(f,"service",f.epoch,1) : f \in sealed}
       IN /\ candidates # {}
          /\ (Attack # "epoch" \/ (current=EpochCount /\ EpochCount>1))
          /\ \E p \in candidates : wire'=wire \cup {p}
    /\ attacked'=TRUE
    /\ UNCHANGED <<current,issued,knows,sent,sealed,handled,accepted,seen,confirmed,confirmWire,exposed>>
Confirm == /\ current \notin confirmed
           /\ current \in confirmWire
           /\ confirmed'=confirmed \cup {current}
           /\ confirmWire'=confirmWire \ {current}
           /\ UNCHANGED <<current,issued,knows,sent,sealed,wire,handled,accepted,seen,exposed,attacked>>
Rotate == /\ current<EpochCount /\ \E n \in Messages : <<current,n>> \in sent
          /\ current'=current+1
          /\ UNCHANGED <<issued,knows,sent,sealed,wire,handled,accepted,seen,confirmed,confirmWire,exposed,attacked>>
Learn == /\ LET readable=={<<f.epoch,f.body>> : f \in {x \in sealed : x.clear \/ x.key \in knows[4]}}
            IN /\ ~(readable \subseteq exposed) /\ exposed'=exposed \cup readable
         /\ UNCHANGED <<current,issued,knows,sent,sealed,wire,handled,accepted,seen,confirmed,confirmWire,attacked>>
GiveCredentials == \E p \in {1,3}, e \in Epochs : Credential(p,e)
EmitAny == \E n \in Messages : Emit(n)
ReceiveAny == \E p \in wire : Receive(p)
Next == Issue \/ GiveCredentials \/ EmitAny \/ ReceiveAny \/ Inject \/ Confirm \/ Rotate \/ Learn
Spec == Init /\ [][Next]_vars
LiveSpec == Spec /\ WF_vars(Issue) /\ WF_vars(GiveCredentials) /\ WF_vars(EmitAny)
            /\ WF_vars(ReceiveAny) /\ WF_vars(Confirm) /\ WF_vars(Rotate)
Frames == [epoch:Epochs,nonce:Messages,key:0..EpochCount,caller:{1},host:{3},
           cap:{"service"},body:1..(MessageCount+1),clear:BOOLEAN]
Packets == [frame:Frames,useCap:{"service","admin"},session:Epochs,copy:{0,1}]
TypeOK == /\ current \in Epochs /\ issued \subseteq Epochs
          /\ knows \in [Principals -> SUBSET (0..EpochCount)]
          /\ sent \subseteq Epochs \X Messages /\ sealed \subseteq Frames
          /\ wire \subseteq Packets /\ handled \subseteq Packets
          /\ accepted \in Seq([frame:Frames,useCap:{"service","admin"},atEpoch:Epochs])
          /\ seen \subseteq (0..EpochCount) \X Messages /\ confirmed \subseteq Epochs
          /\ confirmWire \subseteq Epochs
          /\ exposed \subseteq Epochs \X (1..(MessageCount+1)) /\ attacked \in BOOLEAN
AuthenticatedDelivery == \A i \in 1..Len(accepted) :
    accepted[i].frame \in sealed /\ accepted[i].frame.key \in issued \cap knows[3]
CapabilityBinding == \A i \in 1..Len(accepted) : accepted[i].useCap=accepted[i].frame.cap
FreshSession == \A i \in 1..Len(accepted) : accepted[i].frame.epoch=accepted[i].atEpoch
AtMostOnce == \A i,j \in 1..Len(accepted) :
    <<accepted[i].frame.key,accepted[i].frame.nonce>> = <<accepted[j].frame.key,accepted[j].frame.nonce>> => i=j
NonceUnique == \A f,g \in sealed : <<f.key,f.nonce>>= <<g.key,g.nonce>> => f=g
Confidentiality == exposed={}
Completes == <>(current=EpochCount /\ current \in confirmed /\
    (\A n \in Messages : \E i \in 1..Len(accepted) :
        accepted[i].atEpoch=current /\ accepted[i].frame.body=n))
NoZeroRttWitness == Len(accepted)=0 \/ accepted[Len(accepted)].atEpoch \in confirmed
NoReplayRejectionWitness == ~\E p \in handled : p.copy=1 /\
    (\E q \in handled : q.copy=0 /\ q.frame=p.frame) /\ Len(accepted)=1
NoRotationWitness == ~(current=2 /\ 2 \in confirmed)
=============================================================================
