------------------------- MODULE RpcTailTransfer -------------------------
EXTENDS Naturals, TLC
CONSTANTS DuplicateReturn, DropPipelineOnAck
VARIABLES finished, ack, pipelined, childAck, connected, reused, returns, pipelineLive
vars == <<finished,ack,pipelined,childAck,connected,reused,returns,pipelineLive>>
\* Init follows dispatch: outgoing Call.sendResultsTo.yourself and parent
\* Return.takeFromOtherQuestion refer to the same outgoing question ID.
\* A pipelined child is automatically proxied with its own Call.yourself and
\* Return.takeFromOtherQuestion. Replay checks both matching question IDs and
\* exactly one child Return at Pipeline, then sends ResultsSentElsewhere at Reply.
\* Replay verifies this prefix and released incoming flow credit before any ack.
Init == /\ finished = FALSE /\ ack = FALSE /\ pipelined = FALSE
        /\ childAck = 0 /\ connected = TRUE /\ reused = FALSE
        /\ returns = 1 /\ pipelineLive = TRUE
Acknowledge == /\ connected /\ ~ack /\ ack' = TRUE
               /\ returns' = IF DuplicateReturn THEN returns + 1 ELSE returns
               /\ pipelineLive' = IF DropPipelineOnAck THEN FALSE ELSE pipelineLive
               /\ UNCHANGED <<finished,pipelined,childAck,connected,reused>>
Finish == /\ connected /\ ~finished /\ finished' = TRUE /\ pipelineLive' = FALSE
          /\ UNCHANGED <<ack,pipelined,childAck,connected,reused,returns>>
Pipeline == /\ connected /\ ~finished /\ ~pipelined /\ pipelineLive /\ pipelined' = TRUE
            /\ UNCHANGED <<finished,ack,childAck,connected,reused,returns,pipelineLive>>
Reply == /\ connected /\ pipelined /\ childAck = 0 /\ childAck' = 1
         /\ UNCHANGED <<finished,ack,pipelined,connected,reused,returns,pipelineLive>>
Disconnect == /\ connected /\ connected' = FALSE /\ pipelineLive' = FALSE
              /\ UNCHANGED <<finished,ack,pipelined,childAck,reused,returns>>
Reuse == /\ connected /\ finished /\ ~reused /\ reused' = TRUE
         /\ UNCHANGED <<finished,ack,pipelined,childAck,connected,returns,pipelineLive>>
Next == Acknowledge \/ Finish \/ Pipeline \/ Reply \/ Disconnect \/ Reuse
Spec == Init /\ [][Next]_vars
TypeOK == /\ finished \in BOOLEAN /\ ack \in BOOLEAN /\ pipelined \in BOOLEAN
          /\ childAck \in 0..1 /\ connected \in BOOLEAN /\ reused \in BOOLEAN
          /\ returns \in 1..2 /\ pipelineLive \in BOOLEAN
SingleReturn == returns = 1
PipelineSurvivesAck == (connected /\ ~finished) => pipelineLive
SafeReuse == reused => finished
ChildProvenance == childAck = 1 => pipelined
=============================================================================
