-------------------------- MODULE RpcCancellation --------------------------
EXTENDS Naturals, TLC
CONSTANTS HoldContext, Redirect, BrokenCancellation, BrokenDoubleReturn
VARIABLES finished, held, child, completed, running, connected, answer,
          result, returns, reused, childResult
vars == <<finished, held, child, completed, running, connected, answer,
          result, returns, reused, childResult>>
\* Init is immediately after dispatching a gated server call. A held Results
\* models application code retaining the context independently of its task.
Init == /\ finished = FALSE /\ held = HoldContext /\ child = 0
        /\ completed = FALSE /\ running = TRUE /\ connected = TRUE
        /\ answer = TRUE /\ result = 0 /\ returns = 0 /\ reused = FALSE
        /\ childResult = 0
CancelKind == IF Redirect THEN 3 ELSE 2
Finish == /\ connected /\ ~finished /\ finished' = TRUE
          /\ running' = (running /\ child = 1)
          /\ LET reply == result = 0 /\ ~running' /\ ~held /\ ~BrokenCancellation
             IN /\ result' = IF reply THEN CancelKind ELSE result
                /\ returns' = IF reply THEN returns + 1 ELSE returns
          /\ answer' = (result' = 0)
          /\ UNCHANGED <<held, child, completed, connected, reused, childResult>>
Complete == /\ connected /\ running
            /\ running' = FALSE /\ completed' = TRUE /\ held' = FALSE
            /\ result' = IF Redirect THEN 3 ELSE IF finished THEN 2 ELSE 1
            /\ returns' = IF result = 0 THEN returns + 1 ELSE returns
            /\ answer' = ~finished
            /\ child' = IF child = 1 THEN 2 ELSE child
            /\ childResult' = IF child = 1 THEN 1 ELSE childResult
            /\ UNCHANGED <<finished, connected, reused>>
Pipeline == /\ connected /\ ~finished /\ running /\ child = 0
            /\ child' = 1
            /\ UNCHANGED <<finished, held, completed, running, connected,
                            answer, result, returns, reused, childResult>>
CancelChild == /\ connected /\ child = 1 /\ child' = 3 /\ childResult' = 2
               /\ running' = (running /\ ~finished)
               /\ LET reply == finished /\ ~held /\ result = 0 /\ ~BrokenCancellation
                  IN /\ result' = IF reply THEN CancelKind ELSE result
                     /\ returns' = IF reply THEN returns + 1 ELSE returns
               /\ answer' = (~finished \/ result' = 0)
               /\ UNCHANGED <<finished, held, completed, connected, reused>>
DropContext == /\ held /\ ~running /\ held' = FALSE
               /\ LET reply == connected /\ result = 0 /\ ~BrokenCancellation
                  IN /\ result' = IF reply THEN CancelKind ELSE result
                     /\ returns' = IF reply THEN returns + 1 ELSE returns
               /\ answer' = (connected /\ (~finished \/ result' = 0))
               /\ UNCHANGED <<finished, child, completed, running, connected, reused, childResult>>
Disconnect == /\ connected /\ connected' = FALSE /\ running' = FALSE /\ answer' = FALSE
              /\ child' = IF child = 1 THEN 3 ELSE child
              /\ UNCHANGED <<finished, held, completed, result, returns, reused, childResult>>
Reuse == /\ connected /\ finished /\ ~answer /\ result # 0 /\ ~reused
         /\ reused' = TRUE
         /\ UNCHANGED <<finished, held, child, completed, running, connected,
                         answer, result, returns, childResult>>
RepeatFinish == /\ connected /\ finished
                /\ returns' = IF BrokenDoubleReturn THEN returns + 1 ELSE returns
                /\ UNCHANGED <<finished, held, child, completed, running,
                                connected, answer, result, reused, childResult>>
Next == Finish \/ Complete \/ Pipeline \/ CancelChild \/ DropContext \/ Disconnect \/ Reuse \/ RepeatFinish
Spec == Init /\ [][Next]_vars
TypeOK == /\ finished \in BOOLEAN /\ held \in BOOLEAN /\ completed \in BOOLEAN
          /\ running \in BOOLEAN /\ connected \in BOOLEAN /\ answer \in BOOLEAN
          /\ reused \in BOOLEAN /\ child \in 0..3 /\ childResult \in 0..2
          /\ result \in 0..3 /\ returns \in 0..2
SingleReturn == returns <= 1
CancellationReturn == (connected /\ finished /\ ~held /\ ~running) => result # 0
ContextLifetime == (held /\ ~completed) => result = 0
PipelineLifetime == (connected /\ child = 1) => running
AnswerLifetime == connected => (answer <=> (~finished \/ result = 0))
=============================================================================
