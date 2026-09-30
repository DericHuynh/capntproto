----------------------------- MODULE RpcCallHints -----------------------------
EXTENDS Naturals, TLC
CONSTANTS Params, EarlyFinish, IgnoreFallback, LoseExports
VARIABLES sent, derived, rootDropped, childDropped, used, legacy, second, released,
          finished, secondHigh, alive, secondAfterLegacy
vars == <<sent,derived,rootDropped,childDropped,used,legacy,second,released,
          finished,secondHigh,alive,secondAfterLegacy>>
Init == /\ sent = FALSE /\ derived = FALSE /\ rootDropped = FALSE
        /\ childDropped = FALSE /\ used = FALSE /\ legacy = FALSE /\ second = FALSE
        /\ released = FALSE /\ finished = FALSE /\ secondHigh = 0
        /\ alive = FALSE /\ secondAfterLegacy = FALSE
Send == /\ ~sent /\ sent' = TRUE /\ alive' = Params
        /\ UNCHANGED <<derived,rootDropped,childDropped,used,legacy,second,released,
                       finished,secondHigh,secondAfterLegacy>>
Derive == /\ sent /\ ~rootDropped /\ ~derived /\ derived' = TRUE
          /\ UNCHANGED <<sent,rootDropped,childDropped,used,legacy,second,released,
                         finished,secondHigh,alive,secondAfterLegacy>>
DropRoot == /\ sent /\ ~rootDropped /\ rootDropped' = TRUE
            /\ finished' = (EarlyFinish \/ ~derived \/ childDropped)
            /\ alive' = IF LoseExports /\ finished' THEN FALSE ELSE alive
            /\ UNCHANGED <<sent,derived,childDropped,used,legacy,second,released,secondHigh,secondAfterLegacy>>
DropChild == /\ derived /\ ~childDropped /\ childDropped' = TRUE
             /\ finished' = rootDropped
             /\ alive' = IF LoseExports /\ finished' THEN FALSE ELSE alive
             /\ UNCHANGED <<sent,derived,rootDropped,used,legacy,second,released,secondHigh,secondAfterLegacy>>
Use == /\ derived /\ ~childDropped /\ ~used /\ used' = TRUE
       /\ UNCHANGED <<sent,derived,rootDropped,childDropped,legacy,second,released,
                      finished,secondHigh,alive,secondAfterLegacy>>
\* The Return can arrive even after the high question entry was erased. Its
\* result union is ignored, and subsequent calls use normal low IDs and Returns.
Legacy == /\ sent /\ ~legacy /\ legacy' = TRUE
          /\ UNCHANGED <<sent,derived,rootDropped,childDropped,used,second,released,
                         finished,secondHigh,alive,secondAfterLegacy>>
Second == /\ sent /\ ~second /\ second' = TRUE /\ secondAfterLegacy' = legacy
          /\ secondHigh' = IF IgnoreFallback \/ ~legacy THEN 1 ELSE 0
          /\ UNCHANGED <<sent,derived,rootDropped,childDropped,used,legacy,released,finished,alive>>
\* Export references belong to the receiver's imports, not the retired question.
Release == /\ Params /\ sent /\ ~released /\ released' = TRUE /\ alive' = FALSE
           /\ UNCHANGED <<sent,derived,rootDropped,childDropped,used,legacy,second,
                          finished,secondHigh,secondAfterLegacy>>
Next == Send \/ Derive \/ DropRoot \/ DropChild \/ Use \/ Legacy \/ Second \/ Release
Spec == Init /\ [][Next]_vars
TypeOK == /\ sent \in BOOLEAN /\ derived \in BOOLEAN /\ rootDropped \in BOOLEAN
          /\ childDropped \in BOOLEAN /\ used \in BOOLEAN /\ legacy \in BOOLEAN
          /\ second \in BOOLEAN /\ released \in BOOLEAN /\ finished \in BOOLEAN
          /\ secondHigh \in 0..1 /\ alive \in BOOLEAN /\ secondAfterLegacy \in BOOLEAN
LastOwnerFinish == finished = (sent /\ rootDropped /\ (~derived \/ childDropped))
LegacyFallback == second => secondHigh = (IF secondAfterLegacy THEN 0 ELSE 1)
ExportLifetime == alive = (Params /\ sent /\ ~released)
=============================================================================
