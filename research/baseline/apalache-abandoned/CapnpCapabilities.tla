----------------------- MODULE CapnpCapabilities -----------------------
EXTENDS Naturals, Sequences, FiniteSets, TLC, CapnpComponentTypes
\* Export/import accounting, including crossing Release/Resolve, implicit
\* releases, unimplemented Resolve, promise generations and numeric ID reuse.
CONSTANTS
    \* @type: Int;
    MaxSends,
    \* @type: Str;
    Bug,
    \* @type: Bool;
    AllowDisconnect
Ids == {1, 2}
Tickets == 1..MaxSends
\* @type: $capExport;
Empty == [count |-> 0, generation |-> 0, promise |-> FALSE,
          resolved |-> FALSE, target |-> 0]
VARIABLES
    \* @type: Int -> $capExport;
    exports,
    \* @type: Int -> $capImport;
    imports,
    \* @type: Int -> $capCredit;
    credit,
    \* @type: Int;
    sent,
    \* @type: Seq($capMessage);
    outgoing,
    \* @type: Seq(Set(Int));
    incoming,
    \* @type: Bool;
    connected,
    \* @type: Set(Str);
    faults
\* @type: <<Int -> $capExport, Int -> $capImport, Int -> $capCredit, Int, Seq($capMessage), Seq(Set(Int)), Bool, Set(Str)>>;
vars == <<exports, imports, credit, sent, outgoing, incoming, connected, faults>>
\* @type: (Str, Int, Int, Int, Int) => $capMessage;
Msg(kind, id, generation, ticket, replacement) ==
    [kind |-> kind, id |-> id, generation |-> generation,
     ticket |-> ticket, replacement |-> replacement]
Init == /\ exports = [id \in Ids |-> Empty]
        /\ imports = [id \in Ids |-> [count |-> 0, generation |-> 0, resolved |-> FALSE]]
        /\ credit = [t \in Tickets |-> [id |-> 0, generation |-> 0,
                                      state |-> "unused", source |-> "none"]]
        /\ sent = 0 /\ outgoing = <<>> /\ incoming = <<>>
        /\ connected = TRUE /\ faults = {}

\* One descriptor occurrence is one credit, even when the same ID occurs
\* repeatedly. Payload bytes and local reference copies create no credits.
\* @type: (Int, Bool, Str) => Bool;
Export(id, promise, source) ==
    /\ connected /\ sent < MaxSends
    /\ exports[id].count = 0 \/
       (~exports[id].resolved /\ exports[id].promise = promise)
    /\ LET gen == IF exports[id].count = 0 THEN exports[id].generation + 1
                  ELSE exports[id].generation
           t == sent + 1
       IN /\ exports' = [exports EXCEPT ![id] =
                  [count |-> @.count + 1, generation |-> gen, promise |-> promise,
                   resolved |-> FALSE, target |-> 0]]
          /\ credit' = [credit EXCEPT ![t] =
                  [id |-> id, generation |-> gen, state |-> "outgoing", source |-> source]]
          /\ outgoing' = Append(outgoing, Msg("cap", id, gen, t, 0))
    /\ sent' = sent + 1
    /\ UNCHANGED <<imports, incoming, connected, faults>>

\* Resolve itself adds no reference to promiseId. The replacement descriptor
\* does add one credit. A fixed different ID bounds chains to one link here;
\* immutable multi-link forwarding is checked by CapnpEmbargo.
\* @type: (Int, Int) => Bool;
Resolve(id, replacement) ==
    /\ connected /\ exports[id].count > 0
    /\ exports[id].promise /\ ~exports[id].resolved
    /\ replacement \in {0} \cup (Ids \ {id})
    /\ replacement = 0 \/ sent < MaxSends
    /\ IF replacement = 0
       THEN /\ exports' = [exports EXCEPT ![id].resolved = TRUE]
            /\ outgoing' = Append(outgoing, Msg("resolve", id, exports[id].generation, 0, 0))
            /\ UNCHANGED <<sent, credit>>
       ELSE LET r == replacement
                gen == IF exports[r].count = 0 THEN exports[r].generation + 1
                       ELSE exports[r].generation
                t == sent + 1
            IN /\ ~exports[r].promise \/ exports[r].count = 0
               /\ exports' = [exports EXCEPT ![id].resolved = TRUE, ![id].target = r,
                       ![r] = [count |-> @.count + 1, generation |-> gen,
                               promise |-> FALSE, resolved |-> FALSE, target |-> 0]]
               /\ credit' = [credit EXCEPT ![t] =
                       [id |-> r, generation |-> gen, state |-> "outgoing", source |-> "resolve"]]
               /\ outgoing' = Append(outgoing, Msg("resolve", id, exports[id].generation, t, r))
               /\ sent' = sent + 1
    /\ UNCHANGED <<imports, incoming, connected, faults>>

ReceiveExport ==
    /\ connected /\ Len(outgoing) > 0
    /\ LET m == Head(outgoing)
           known == imports[m.id].count > 0 /\ imports[m.id].generation = m.generation
       IN /\ IF m.kind = "cap"
             THEN /\ imports' = [imports EXCEPT ![m.id] =
                          [count |-> @.count + 1, generation |-> m.generation, resolved |-> FALSE]]
                  /\ credit' = [credit EXCEPT ![m.ticket].state = "held"]
                  /\ UNCHANGED incoming
             ELSE /\ \E supported \in BOOLEAN :
                       LET discard == ~known \/ ~supported
                       IN /\ imports' = [i \in Ids |->
                               IF i = m.id /\ known /\ supported
                               THEN [imports[i] EXCEPT !.resolved = TRUE]
                               ELSE IF i = m.replacement /\ ~discard
                               THEN [imports[i] EXCEPT !.count = @ + 1,
                                          !.generation = credit[m.ticket].generation]
                               ELSE imports[i]]
                          /\ credit' = IF m.ticket = 0 THEN credit
                               ELSE [credit EXCEPT ![m.ticket].state =
                                      IF discard THEN "releasing" ELSE "held"]
                          /\ incoming' = IF m.ticket # 0 /\ discard
                               THEN Append(incoming, {m.ticket}) ELSE incoming
          /\ faults' = IF m.kind = "cap" /\ imports[m.id].count > 0 /\ ~known
                       THEN faults \cup {"staleId"} ELSE faults
    /\ outgoing' = Tail(outgoing)
    /\ UNCHANGED <<exports, sent, connected>>

\* Return.releaseParamCaps and Finish.releaseResultCaps release each original
\* descriptor exactly once. Explicit Release may batch credits of one ID.
\* Local ownership determines the choice; a credit cannot be released twice.
\* @type: (Int, Set(Int), Bool) => Bool;
Release(id, ts, implicit) ==
    /\ connected /\ ts # {} /\ ts \subseteq Tickets
    /\ \A t \in ts : credit[t].id = id /\ credit[t].state = "held"
    /\ implicit => (\A t \in ts : credit[t].source \in {"params", "results"})
    /\ imports' = [imports EXCEPT ![id].count = @ - Cardinality(ts)]
    /\ credit' = [t \in Tickets |-> IF t \in ts
                    THEN [credit[t] EXCEPT !.state = "releasing"] ELSE credit[t]]
    /\ incoming' = Append(incoming, ts)
    /\ UNCHANGED <<exports, sent, outgoing, connected, faults>>

ReceiveRelease ==
    /\ connected /\ Len(incoming) > 0
    /\ LET ts == Head(incoming)
       IN /\ exports' = [id \in Ids |-> [exports[id] EXCEPT !.count = @ -
                            Cardinality({t \in ts : credit[t].id = id})]]
          /\ credit' = [t \in Tickets |-> IF t \in ts
                         THEN [credit[t] EXCEPT !.state = "released"] ELSE credit[t]]
          /\ faults' = IF \A t \in ts : credit[t].generation = exports[credit[t].id].generation
                       THEN faults ELSE faults \cup {"staleRelease"}
    /\ incoming' = Tail(incoming)
    /\ UNCHANGED <<imports, sent, outgoing, connected>>

\* Negative control: importer zero does not imply exporter zero; a fresh
\* descriptor can still be in flight in the other direction.
\* @type: Int => Bool;
PrematureFree(id) ==
    /\ Bug = "freeOnImporterZero" /\ connected
    /\ imports[id].count = 0 /\ exports[id].count > 0
    /\ exports' = [exports EXCEPT ![id].count = 0]
    /\ UNCHANGED <<imports, credit, sent, outgoing, incoming, connected, faults>>
Disconnect == /\ connected /\ AllowDisconnect /\ connected' = FALSE
              /\ exports' = [id \in Ids |-> Empty]
              /\ imports' = [id \in Ids |-> [count |-> 0, generation |-> 0, resolved |-> FALSE]]
              /\ credit' = [t \in Tickets |-> [credit[t] EXCEPT !.state = "released"]]
              /\ outgoing' = <<>> /\ incoming' = <<>>
              /\ UNCHANGED <<sent, faults>>
Next == \/ \E id \in Ids, p \in BOOLEAN, src \in {"params", "results"} : Export(id, p, src)
        \/ \E id \in Ids, r \in {0} \cup Ids : Resolve(id, r)
        \/ ReceiveExport \/ ReceiveRelease \/ Disconnect
        \/ \E id \in Ids, ts \in SUBSET Tickets, implicit \in BOOLEAN : Release(id, ts, implicit)
        \/ \E id \in Ids : PrematureFree(id)
Spec == Init /\ [][Next]_vars
TypeOK == /\ connected \in BOOLEAN /\ sent \in 0..MaxSends
          /\ \A id \in Ids : exports[id].count \in 0..MaxSends /\ imports[id].count \in 0..MaxSends
          /\ \A t \in Tickets : credit[t].state \in
                  {"unused", "outgoing", "held", "releasing", "released"}
Conservation == \A id \in Ids :
    /\ exports[id].count = Cardinality({t \in Tickets : credit[t].id = id /\
                         credit[t].state \in {"outgoing", "held", "releasing"}})
    /\ imports[id].count = Cardinality({t \in Tickets : credit[t].id = id /\ credit[t].state = "held"})
NoStaleGeneration == connected => \A t \in Tickets :
    credit[t].state \in {"outgoing", "held", "releasing"} =>
       credit[t].generation = exports[credit[t].id].generation
ProtocolSafety == faults = {}
NoReleasedResolveWitness == ~\E t \in Tickets : credit[t].source = "resolve" /\ credit[t].state = "released"
NoExportReuseWitness == ~\E id \in Ids : exports[id].generation > 1
=============================================================================
