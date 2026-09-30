------------------------- MODULE RpcSchemaHints -------------------------
EXTENDS Naturals, FiniteSets, TLC
CONSTANTS MissLeaf, StopEarly
VARIABLES graph, seeds, visited, pending, found, emitted, hint
vars == <<graph,seeds,visited,pending,found,emitted,hint>>
Nodes == 0..2
Has(mask, bit) == (mask \div (2^bit)) % 2 = 1
Decode(mask) == {i \in Nodes: Has(mask,i)}
Encode(set) == (IF 0 \in set THEN 1 ELSE 0) +
               (IF 1 \in set THEN 2 ELSE 0) + (IF 2 \in set THEN 4 ELSE 0)
Neighbors(i) == {j \in Nodes: Has(graph, 3*i+j)}
Reach1 == {0} \cup Neighbors(0)
Reach2 == Reach1 \cup UNION {Neighbors(i): i \in Reach1}
MayContainCaps == \E i \in Reach2: Has(seeds,i)
Init == /\ graph \in 0..511 /\ seeds \in 0..7 /\ visited = 0
        /\ pending = 1 /\ found = FALSE /\ emitted = FALSE /\ hint = FALSE
Visit(i) == /\ ~emitted /\ ~found /\ i \in Decode(pending)
            /\ visited' = Encode(Decode(visited) \cup {i})
            /\ pending' = Encode((Decode(pending) \ {i}) \cup (Neighbors(i) \ Decode(visited')))
            /\ found' = (found \/ (Has(seeds,i) /\ ~(MissLeaf /\ i=2)))
            /\ UNCHANGED <<graph,seeds,emitted,hint>>
Emit == /\ ~emitted /\ (found \/ pending=0 \/ (StopEarly /\ visited # 0))
        /\ emitted' = TRUE /\ hint' = ~found
        /\ UNCHANGED <<graph,seeds,visited,pending,found>>
Next == (\E i \in Nodes: Visit(i)) \/ Emit
Spec == Init /\ [][Next]_vars
TypeOK == /\ graph \in 0..511 /\ seeds \in 0..7 /\ visited \in 0..7 /\ pending \in 0..7
          /\ found \in BOOLEAN /\ emitted \in BOOLEAN /\ hint \in BOOLEAN
CorrectHint == emitted => hint = ~MayContainCaps
=============================================================================
