----------------------- MODULE CapnpTwoPartyJoin -----------------------
EXTENDS Naturals, Sequences, FiniteSets, TLC, CapnpComponentTypes
\* rpc-twoparty.capnp: collect all parts at a network boundary before starting
\* a new join across it. All replies agree; exactly one carries the capability.
CONSTANTS
    \* @type: Bool;
    SameImport,
    \* @type: Bool;
    EqualObjects
ASSUME SameImport => EqualObjects
Parts == {0, 1}
VARIABLES
    \* @type: Set(Int);
    received,
    \* @type: Set(Int);
    forwarded,
    \* @type: Set(Int);
    remoteParts,
    \* @type: Seq($twoPartyReply);
    replies,
    \* @type: Seq($twoPartyReply);
    observed,
    \* @type: Set(Int);
    finished,
    \* @type: Bool;
    localDone,
    \* @type: Bool;
    remoteDone,
    \* @type: Bool;
    succeeded,
    \* @type: Bool;
    released
\* @type: <<Set(Int), Set(Int), Set(Int), Seq($twoPartyReply), Seq($twoPartyReply), Set(Int), Bool, Bool, Bool, Bool>>;
vars == <<received, forwarded, remoteParts, replies, observed, finished,
          localDone, remoteDone, succeeded, released>>
Init == /\ received = {} /\ forwarded = {} /\ remoteParts = {}
        /\ replies = <<>> /\ observed = <<>> /\ finished = {}
        /\ localDone = FALSE /\ remoteDone = FALSE /\ succeeded = FALSE /\ released = FALSE
ReceivePart(p) ==
    /\ p \notin received /\ received' = received \cup {p}
    /\ UNCHANGED <<forwarded, remoteParts, replies, observed, finished,
                   localDone, remoteDone, succeeded, released>>
LocalEquality ==
    /\ received = Parts /\ SameImport /\ ~localDone
    /\ localDone' = TRUE /\ succeeded' = TRUE
    /\ replies' = <<[part |-> 0, succeeded |-> TRUE, cap |-> TRUE],
                    [part |-> 1, succeeded |-> TRUE, cap |-> FALSE]>>
    /\ UNCHANGED <<received, forwarded, remoteParts, observed, finished, remoteDone, released>>
ForwardPart(p) ==
    /\ received = Parts /\ ~SameImport /\ p \notin forwarded
    /\ forwarded' = forwarded \cup {p}
    /\ UNCHANGED <<received, remoteParts, replies, observed, finished,
                   localDone, remoteDone, succeeded, released>>
RemoteReceive(p) ==
    /\ p \in forwarded \ remoteParts /\ remoteParts' = remoteParts \cup {p}
    /\ UNCHANGED <<received, forwarded, replies, observed, finished,
                   localDone, remoteDone, succeeded, released>>
RemoteJoin ==
    /\ remoteParts = Parts /\ ~remoteDone
    /\ remoteDone' = TRUE /\ succeeded' = EqualObjects
    /\ replies' = <<[part |-> 0, succeeded |-> EqualObjects, cap |-> EqualObjects],
                    [part |-> 1, succeeded |-> EqualObjects, cap |-> FALSE]>>
    /\ UNCHANGED <<received, forwarded, remoteParts, observed, finished, localDone, released>>
ReceiveResult ==
    /\ Len(replies) > 0 /\ observed' = Append(observed, Head(replies))
    /\ replies' = Tail(replies)
    /\ UNCHANGED <<received, forwarded, remoteParts, finished, localDone, remoteDone, succeeded, released>>
Finish(p) ==
    /\ Len(observed) = 2 /\ p \notin finished /\ finished' = finished \cup {p}
    /\ UNCHANGED <<received, forwarded, remoteParts, replies, observed,
                   localDone, remoteDone, succeeded, released>>
ReleaseJoinId ==
    /\ finished = Parts /\ ~released /\ released' = TRUE
    /\ UNCHANGED <<received, forwarded, remoteParts, replies, observed, finished,
                   localDone, remoteDone, succeeded>>
Next == (\E p \in Parts : ReceivePart(p) \/ ForwardPart(p) \/ RemoteReceive(p) \/ Finish(p)) \/
        LocalEquality \/ RemoteJoin \/ ReceiveResult \/ ReleaseJoinId
Spec == Init /\ [][Next]_vars
CollectBeforeForward == forwarded # {} => received = Parts
Agreement == \A i \in 1..Len(observed) : observed[i].succeeded = EqualObjects
SingleCapability == Len(observed) = 2 =>
    Cardinality({i \in 1..2 : observed[i].cap}) = IF EqualObjects THEN 1 ELSE 0
NoPrematureReuse == released => finished = Parts
NoTwoPartyJoinWitness == ~(released /\ Len(observed) = 2)
=============================================================================
