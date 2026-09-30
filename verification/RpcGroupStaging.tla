------------------------- MODULE RpcGroupStaging -------------------------
EXTENDS Naturals, TLC
CONSTANTS PublishEarly, DamageOnError, LoseHeld, LeakCandidate,
          OverwriteSibling, CopyPayload
VARIABLES phase, candidate, scalar, held, oldAlive, oneAlive, twoAlive,
          madeOne, madeTwo, tag, published, publishedScalar, sibling,
          retained, location, event
vars == <<phase,candidate,scalar,held,oldAlive,oneAlive,twoAlive,madeOne,
          madeTwo,tag,published,publishedScalar,sibling,retained,location,event>>
Init == /\ phase=0 /\ candidate=0 /\ scalar=0 /\ held=0
        /\ oldAlive=TRUE /\ oneAlive=FALSE /\ twoAlive=FALSE
        /\ madeOne=FALSE /\ madeTwo=FALSE /\ tag=0 /\ published=0
        /\ publishedScalar=9 /\ sibling=1 /\ retained=FALSE
        /\ location=0 /\ event=0
Stage == /\ phase=0 /\ phase'=1 /\ event'=1
         /\ UNCHANGED <<candidate,scalar,held,oldAlive,oneAlive,twoAlive,
              madeOne,madeTwo,tag,published,publishedScalar,sibling,retained,location>>
Write == /\ phase=1 /\ scalar<2 /\ scalar'=scalar+1 /\ event'=2
         /\ tag'=(IF PublishEarly THEN 1 ELSE tag)
         /\ UNCHANGED <<phase,candidate,held,oldAlive,oneAlive,twoAlive,
              madeOne,madeTwo,published,publishedScalar,sibling,retained,location>>
First == /\ phase=1 /\ ~madeOne /\ candidate=0
         /\ candidate'=1 /\ oneAlive'=TRUE /\ madeOne'=TRUE /\ event'=3
         /\ UNCHANGED <<phase,scalar,held,oldAlive,twoAlive,madeTwo,tag,
              published,publishedScalar,sibling,retained,location>>
Second == /\ phase=1 /\ candidate=1 /\ ~madeTwo
          /\ candidate'=2 /\ oneAlive'=(held=1) /\ twoAlive'=TRUE
          /\ madeTwo'=TRUE /\ event'=4
          /\ UNCHANGED <<phase,scalar,held,oldAlive,madeOne,tag,published,
               publishedScalar,sibling,retained,location>>
Hold == /\ held=0 /\ ~retained
        /\ ((phase=1 /\ candidate#0) \/ (phase=2 /\ published#0))
        /\ held'=(IF phase=1 THEN candidate ELSE published)
        /\ retained'=TRUE /\ event'=5
        /\ UNCHANGED <<phase,candidate,scalar,oldAlive,oneAlive,twoAlive,
             madeOne,madeTwo,tag,published,publishedScalar,sibling,location>>
Release == /\ held#0 /\ held'=0 /\ event'=6
           /\ oneAlive'=((phase=1 /\ candidate=1) \/ (phase=2 /\ published=1))
           /\ twoAlive'=((phase=1 /\ candidate=2) \/ (phase=2 /\ published=2))
           /\ UNCHANGED <<phase,candidate,scalar,oldAlive,madeOne,madeTwo,
                tag,published,publishedScalar,sibling,retained,location>>
Commit == /\ phase=1 /\ phase'=2 /\ tag'=1 /\ oldAlive'=FALSE
          /\ published'=candidate /\ candidate'=0 /\ publishedScalar'=scalar
          /\ sibling'=(IF OverwriteSibling THEN 0 ELSE sibling)
          /\ location'=(IF CopyPayload THEN 2 ELSE 1) /\ event'=7
          /\ UNCHANGED <<scalar,held,oneAlive,twoAlive,madeOne,madeTwo,retained>>
Discard(kind) == /\ phase=1 /\ phase'=3 /\ candidate'=0 /\ event'=kind
                 /\ oldAlive'=~DamageOnError
                 /\ oneAlive'=((held=1 /\ ~LoseHeld) \/ (candidate=1 /\ LeakCandidate))
                 /\ twoAlive'=((held=2 /\ ~LoseHeld) \/ (candidate=2 /\ LeakCandidate))
                 /\ UNCHANGED <<scalar,held,madeOne,madeTwo,tag,published,
                      publishedScalar,sibling,retained,location>>
Clear == /\ phase\in {2,3} /\ phase'=4 /\ tag'=2
         /\ oldAlive'=FALSE /\ oneAlive'=(held=1) /\ twoAlive'=(held=2)
         /\ published'=0 /\ event'=10
         /\ UNCHANGED <<candidate,scalar,held,madeOne,madeTwo,publishedScalar,
              sibling,retained,location>>
Next == Stage \/ Write \/ First \/ Second \/ Hold \/ Release \/ Commit
        \/ Discard(8) \/ Discard(9) \/ Clear
Spec == Init /\ [][Next]_vars
TypeOK == /\ phase\in 0..4 /\ candidate\in 0..2 /\ scalar\in 0..2
          /\ held\in 0..2 /\ oldAlive\in BOOLEAN /\ oneAlive\in BOOLEAN
          /\ twoAlive\in BOOLEAN /\ madeOne\in BOOLEAN /\ madeTwo\in BOOLEAN
          /\ tag\in 0..2 /\ published\in 0..2 /\ publishedScalar\in (0..2)\cup{9}
          /\ sibling\in 0..1 /\ retained\in BOOLEAN /\ location\in 0..2
          /\ event\in 0..10
NoEarlyPublication == phase\in {0,1,3} => (tag=0 /\ publishedScalar=9 /\ oldAlive)
Ownership == /\ (oneAlive <=> ((phase=1 /\ candidate=1) \/ (phase=2 /\ published=1) \/ held=1))
             /\ (twoAlive <=> ((phase=1 /\ candidate=2) \/ (phase=2 /\ published=2) \/ held=2))
Publication == phase=2 => (tag=1 /\ publishedScalar=scalar /\ ~oldAlive /\ location=1)
SiblingIsolation == sibling=1
=============================================================================
