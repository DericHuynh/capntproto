------------------------- MODULE RpcWireHandoff -------------------------
EXTENDS Naturals, TLC
CONSTANT BrokenEmbargo
VARIABLES phase, requested, released, accepted, failed, old, queued, delivered
vars == <<phase, requested, released, accepted, failed, old, queued, delivered>>
Init == /\ phase = 0 /\ requested = FALSE /\ released = FALSE
        /\ accepted = FALSE /\ failed = FALSE /\ old = FALSE
        /\ queued = FALSE /\ delivered = FALSE
Provide == /\ phase = 0 /\ phase' = 1
           /\ accepted' = (requested /\ BrokenEmbargo)
           /\ delivered' = (queued /\ accepted')
           /\ UNCHANGED <<requested, released, failed, old, queued>>
Accept == /\ ~requested /\ phase < 2 /\ requested' = TRUE
          /\ accepted' = (phase = 1 /\ (released \/ BrokenEmbargo))
          /\ UNCHANGED <<phase, released, failed, old, queued, delivered>>
Release == /\ phase = 1 /\ ~released /\ released' = TRUE
           /\ accepted' = requested
           /\ delivered' = (queued /\ requested)
           /\ UNCHANGED <<phase, requested, failed, old, queued>>
OldCall == /\ phase < 2 /\ ~released /\ ~old /\ old' = TRUE
           /\ UNCHANGED <<phase, requested, released, accepted, failed, queued, delivered>>
NewCall == /\ requested /\ ~queued /\ queued' = TRUE
           /\ delivered' = accepted
           /\ UNCHANGED <<phase, requested, released, accepted, failed, old>>
Finish == /\ phase = 1 /\ phase' = 2
          /\ failed' = (requested /\ ~accepted)
          /\ UNCHANGED <<requested, released, accepted, old, queued, delivered>>
Next == Provide \/ Accept \/ Release \/ OldCall \/ NewCall \/ Finish
Spec == Init /\ [][Next]_vars
TypeOK == /\ phase \in 0..2
          /\ requested \in BOOLEAN /\ released \in BOOLEAN
          /\ accepted \in BOOLEAN /\ failed \in BOOLEAN
          /\ old \in BOOLEAN /\ queued \in BOOLEAN /\ delivered \in BOOLEAN
Embargo == accepted => released
CapabilityAuthority == delivered => accepted /\ requested /\ queued
NoResurrection == failed => ~accepted /\ ~delivered
=============================================================================
