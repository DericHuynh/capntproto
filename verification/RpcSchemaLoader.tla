----------------------------- MODULE RpcSchemaLoader -----------------------------
EXTENDS Naturals, TLC
CONSTANTS PartialCommit, MissingStub, AllowDowngrade, WrongKind
VARIABLES root, dependency, previousRoot, previousDependency, failed, event, steps
vars == <<root, dependency, previousRoot, previousDependency, failed, event, steps>>
Init == /\ root = 0 /\ dependency = 0 /\ previousRoot = 0 /\ previousDependency = 0
        /\ failed = FALSE /\ event = 0 /\ steps = 0
Maximum(a,b) == IF a > b THEN a ELSE b
Record(e) == /\ event' = e /\ steps' = steps + 1
             /\ previousRoot' = root /\ previousDependency' = dependency
Load(version, once) ==
    /\ steps < 3
    /\ root' = IF once /\ root # 0 THEN root
               ELSE IF AllowDowngrade THEN version ELSE Maximum(root,version)
    /\ dependency' = IF MissingStub THEN dependency ELSE Maximum(dependency,1)
    /\ failed' = FALSE
    /\ Record(IF once THEN 6 ELSE version)
LoadDependency == /\ steps < 3 /\ dependency' = 2 /\ UNCHANGED root
                  /\ failed' = FALSE /\ Record(3)
Reject(e) == /\ steps < 3 /\ e \in {4,5,7}
             /\ root' = IF PartialCommit /\ e = 7 THEN 1 ELSE root
             /\ dependency' = IF WrongKind /\ e = 4 THEN 3 ELSE dependency
             /\ failed' = TRUE /\ Record(e)
Next == Load(1,FALSE) \/ Load(2,FALSE) \/ Load(1,TRUE) \/ LoadDependency \/ (\E e \in {4,5,7}: Reject(e))
Spec == Init /\ [][Next]_vars
TypeOK == /\ root \in 0..2 /\ dependency \in 0..3 /\ previousRoot \in 0..2 /\ previousDependency \in 0..3
          /\ failed \in BOOLEAN /\ event \in 0..7 /\ steps \in 0..3
AtomicFailure == failed => (root = previousRoot /\ dependency = previousDependency)
DependencyClosure == root # 0 => dependency # 0
MonotonicVersion == root >= previousRoot
TypedDependencies == dependency # 3
=============================================================================
