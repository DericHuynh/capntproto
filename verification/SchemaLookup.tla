------------------------- MODULE SchemaLookup -------------------------
EXTENDS Naturals
CONSTANT Fault
\* Six finite inheritance graphs: colliding names, an own override, an excessive
\* first branch before a matching sibling, a diamond, and repeated-branch graphs
\* requiring 63 and 127 visits. One query follows declaration-order DFS with the
\* C++ shared 64-visit budget. A base-9 integer encodes at most eight pending nodes.
VARIABLES graph, op, needle, stack, visits, status, result
vars == <<graph,op,needle,stack,visits,status,result>>
First(g,n) == CASE g\in {1,2,4} -> IF n=1 THEN 2 ELSE IF n=2 \/ (g=4 /\ n=3) THEN 4 ELSE 0
                   [] g=3 -> IF n=1 THEN 2 ELSE IF n<7 THEN n+1 ELSE 0
                   [] OTHER -> IF n<g+1 THEN n+1 ELSE 0
Second(g,n) == IF g\in {1,2,4} THEN IF n=1 THEN 3 ELSE 0
              ELSE IF g=3 THEN IF n=1 THEN 8 ELSE IF n<7 THEN n+1 ELSE 0
              ELSE IF g\in {5,6} /\ n<g+1 THEN n+1 ELSE 0
Named(g,n) == (g\in {1,2} /\ n\in {3,4}) \/ (g=2 /\ n=1) \/ (g=4 /\ n=4) \/ (g=3 /\ n=8)
Matches(g,o,q,n) == IF o=0 THEN q=1 /\ Named(g,n) ELSE q=n
Expected(g,o,q) == IF o=0 THEN
    IF q=1 /\ g\in {1,2,4} THEN IF g=2 THEN 1 ELSE 4
    ELSE IF g\in {3,6} THEN 9 ELSE 0
  ELSE IF g\in {1,2,4} THEN IF q\in 1..4 THEN q ELSE 0
       ELSE IF g=3 THEN IF q\in 1..7 THEN q ELSE 9
       ELSE IF g=5 THEN IF q\in 1..6 THEN q ELSE 0
       ELSE IF q\in 1..7 THEN q ELSE 9
Init == /\ graph=0 /\ op=0 /\ needle=0 /\ stack=0 /\ visits=0 /\ status=0 /\ result=0
Begin(g,o,q) == /\ status=0 /\ graph'=g /\ op'=o /\ needle'=q
                /\ stack'=1 /\ visits'=0 /\ status'=1 /\ result'=0
Walk == /\ status=1
        /\ UNCHANGED <<graph,op,needle>>
        /\ IF stack=0 THEN /\ status'=2 /\ result'=0 /\ UNCHANGED <<stack,visits>>
           ELSE IF visits=(IF Fault="earlyLimit" THEN 63 ELSE 64) THEN
               /\ status'=2 /\ result'=IF Fault="swallowLimit" THEN 0 ELSE 9
               /\ UNCHANGED <<stack,visits>>
           ELSE LET n == stack % 9
                    tail == stack \div 9
                    a == First(graph,n)
                    b == Second(graph,n)
                IN /\ visits'=visits+1
                   /\ IF Matches(graph,op,needle,n) /\ ~(Fault="skipSelf" /\ visits=0)
                      THEN /\ status'=2 /\ result'=IF Fault="wrongOwner" THEN 1 ELSE n
                           /\ stack'=tail
                      ELSE /\ status'=1 /\ result'=0
                           /\ stack'=IF a=0 THEN tail ELSE IF b=0 THEN tail*9+a
                               ELSE IF Fault="reverseBranches" THEN tail*81+a*9+b
                               ELSE tail*81+b*9+a
Next == (\E g\in 1..6, o\in 0..1, q\in 0..8: (o=1 \/ q<=1) /\ Begin(g,o,q)) \/ Walk
Spec == Init /\ [][Next]_vars
TypeOK == /\ graph\in 0..6 /\ op\in 0..1 /\ needle\in 0..8
          /\ stack\in 0..43046720 /\ visits\in 0..64 /\ status\in 0..2 /\ result\in 0..9
LookupResult == status=2 => result=Expected(graph,op,needle)
ExactBudget == status=2 /\ result=9 => visits=64
=============================================================================
