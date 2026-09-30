---------------------- MODULE RpcResultOwnership ----------------------
EXTENDS Naturals, FiniteSets
CONSTANT Fault
\* Scoped result construction: one root, one detached owner, two capabilities.
\* 0 = empty; orphan 3 = a typed null owner. Root adoption moves existing arena
\* storage. Copies share capability identity but own independent pointer slots.
\* Four operations per trace; RPC completion, hints, and membranes are native
\* test scope. No executor or unbounded allocator proof is claimed.
VARIABLES root, orphan, live, foreign, beforeRoot, beforeOrphan, steps, event
vars == <<root,orphan,live,foreign,beforeRoot,beforeOrphan,steps,event>>
Held(r,o) == Cardinality({r,o} \ {0,3})
Init == /\ root=0 /\ orphan=0 /\ live=0 /\ foreign=0
        /\ beforeRoot=0 /\ beforeOrphan=0 /\ steps=0 /\ event=0
Update(r,o,e) == /\ root'=r /\ orphan'=o /\ event'=e /\ steps'=steps+1
                 /\ beforeRoot'=root /\ beforeOrphan'=orphan
                 /\ live'=(IF Fault="loseCapability" /\ e=3 THEN 0 ELSE Held(r,o))
                 /\ foreign'=IF Fault="allowForeign" /\ e=7 THEN 1 ELSE foreign
New(k) == /\ orphan=0 /\ root#k /\ Update(root,k,k)
Adopt == /\ orphan#0 /\ Update(IF orphan=3 THEN 0 ELSE orphan,
                            IF Fault="duplicateOwner" THEN orphan ELSE 0,3)
Disown == /\ orphan=0 /\ root#0 /\ Update(0,root,4)
Drop == /\ orphan#0 /\ Update(root,0,5)
Clear == /\ root#0 /\ Update(0,orphan,6)
Reject(e) == /\ orphan#0 /\ Update(root,IF Fault="consumeOnError" THEN 0 ELSE orphan,e)
Copy == /\ orphan=0 /\ root#0 /\ Update(root,root,9)
Null == /\ orphan=0 /\ Update(root,3,10)
Next == /\ steps<4
        /\ (New(1) \/ New(2) \/ Adopt \/ Disown \/ Drop \/ Clear \/ Reject(7) \/ Reject(8) \/ Copy \/ Null)
Spec == Init /\ [][Next]_vars
TypeOK == /\ root\in 0..2 /\ orphan\in 0..3 /\ live\in 0..2
          /\ foreign\in 0..1 /\ beforeRoot\in 0..2 /\ beforeOrphan\in 0..3
          /\ steps\in 0..4 /\ event\in 0..10
CapabilityOwnership == live=Held(root,orphan)
AdoptionMoves == event=3 => orphan=0
FailurePreserves == event\in {7,8} => root=beforeRoot /\ orphan=beforeOrphan /\ foreign=0
=============================================================================
