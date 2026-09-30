------------------------------ MODULE AnyStruct ------------------------------
EXTENDS Naturals
CONSTANT Fault
\* A schema-free struct with 0..2 data words and 0..2 pointer slots.
\* First/last data byte mutations, null/data/capability slots, independent copy,
\* source clear, and canonicalization. Each scenario has at most four actions.
\* Capabilities are opaque authority: copy preserves a callable reference;
\* canonicalization must reject them. No distributed identity claim is made.
VARIABLES allocated, d, p, a, b, x, y, copied, snapshot,
          seenD, seenP, seenValue, seenCopy, canon, canonD, canonP, steps, event
vars == <<allocated,d,p,a,b,x,y,copied,snapshot,seenD,seenP,seenValue,seenCopy,
          canon,canonD,canonP,steps,event>>
Value(aa,bb,xx,yy) == 1000*aa+100*bb+10*xx+yy
CopyValue(dd,pp,aa,bb,xx,yy) == 100000+10000*dd+1000*pp+100*aa+10*bb+3*xx+yy
\* CopyValue uses small nonoverlapping ranges except pointer digits: for
\* x,y in 0..2 the pair 3*x+y is a base-three code.
CanonicalD == IF a+b=0 THEN 0 ELSE IF b#0 THEN d ELSE 1
CanonicalP == IF y#0 THEN 2 ELSE IF x#0 THEN 1 ELSE 0
Init == /\ allocated=0 /\ d=0 /\ p=0 /\ a=0 /\ b=0 /\ x=0 /\ y=0
        /\ copied=0 /\ snapshot=0 /\ seenD=0 /\ seenP=0 /\ seenValue=0 /\ seenCopy=0
        /\ canon=0 /\ canonD=0 /\ canonP=0 /\ steps=0 /\ event=0
Allocate(dd,pp) ==
 /\ allocated=0 /\ copied=0 /\ steps=0
 /\ allocated'=1 /\ d'=dd /\ p'=pp /\ event'=1+3*dd+pp
 /\ UNCHANGED <<a,b,x,y,copied,snapshot>>
WriteFirst == /\ allocated=1 /\ d>0 /\ a'=1 /\ event'=20
              /\ UNCHANGED <<allocated,d,p,b,x,y,copied,snapshot>>
WriteLast == /\ allocated=1 /\ d>0 /\ b'=2 /\ event'=21
             /\ UNCHANGED <<allocated,d,p,a,x,y,copied,snapshot>>
Pointer(slot,value) ==
 /\ allocated=1 /\ p>slot /\ event'=30+slot*3+value
 /\ x'=(IF slot=0 THEN value ELSE x)
 /\ y'=(IF slot=1 THEN value ELSE y)
 /\ UNCHANGED <<allocated,d,p,a,b,copied,snapshot>>
Copy == /\ allocated=1 /\ event'=40
        /\ snapshot'=CopyValue(d,p,a,b,x,y)
        /\ copied'=CopyValue(d,p,a,b,IF Fault="dropCapabilities" /\ x=2 THEN 0 ELSE x,
                                      IF Fault="dropCapabilities" /\ y=2 THEN 0 ELSE y)
        /\ UNCHANGED <<allocated,d,p,a,b,x,y>>
Clear == /\ allocated=1 /\ event'=41 /\ allocated'=0 /\ d'=0 /\ p'=0
         /\ a'=0 /\ b'=0 /\ x'=0 /\ y'=0 /\ UNCHANGED <<copied,snapshot>>
Canonicalize == /\ allocated=1 /\ event'=42
                /\ UNCHANGED <<allocated,d,p,a,b,x,y,copied,snapshot>>
Observe ==
 /\ seenD'=IF Fault="truncateData" /\ d'=2 THEN 1 ELSE d'
 /\ seenP'=IF Fault="truncatePointers" /\ p'=2 THEN 1 ELSE p'
 /\ seenValue'=IF Fault="swapSlots" THEN Value(a',b',y',x') ELSE Value(a',b',x',y')
 /\ seenCopy'=IF Fault="aliasCopy" /\ copied'#0 THEN CopyValue(d',p',a',b',x',y') ELSE copied'
 /\ canon'=IF event'#42 THEN 0 ELSE IF Fault#"canonicalCaps" /\ (x=2 \/ y=2) THEN 2 ELSE 1
 /\ canonD'=IF event'#42 \/ canon'=2 THEN 0 ELSE IF Fault="canonicalPadding" THEN d ELSE CanonicalD
 /\ canonP'=IF event'#42 \/ canon'=2 THEN 0 ELSE CanonicalP
Next == /\ steps<4 /\ steps'=steps+1
        /\ ((\E dd,pp\in 0..2: Allocate(dd,pp)) \/ WriteFirst \/ WriteLast
            \/ (\E slot\in 0..1,value\in 0..2: Pointer(slot,value)) \/ Copy \/ Clear \/ Canonicalize)
        /\ Observe
Spec == Init /\ [][Next]_vars
TypeOK == /\ allocated\in 0..1 /\ d\in 0..2 /\ p\in 0..2 /\ a\in 0..1 /\ b\in {0,2}
          /\ x\in 0..2 /\ y\in 0..2 /\ canon\in 0..2 /\ steps\in 0..4
Sections == seenD=d /\ seenP=p /\ seenValue=Value(a,b,x,y)
IndependentCopy == seenCopy=snapshot
CanonicalContract == event=42 =>
   /\ canon=(IF x=2 \/ y=2 THEN 2 ELSE 1)
   /\ (canon=1 => canonD=CanonicalD /\ canonP=CanonicalP)
=============================================================================
