--------------------------- MODULE StructuralEquality ---------------------------
EXTENDS Naturals, Sequences, FiniteSets
CONSTANT Fault
\* Bounded structural comparison, NOT distributed capability Join. Scalar IDs
\* below denote acyclic wire values with nulls, capabilities, padded structs,
\* all list categories, and two-element aggregates. Rust constructs their wire
\* layouts independently and replays each explored edge prefix, then compares
\* terminal observations with the pinned C++ AnyPointer::equals implementation.
\* 1=equal, 2=different, 3=unknown capabilities; errors/limits/far pointers are
\* covered by native and C++ regressions rather than this finite value algebra.
VARIABLES phase, left, right, result, event
vars == <<phase,left,right,result,event>>
Value(k,d,p,e,n) == [kind|->k,data|->d,ptrs|->p,encoding|->e,count|->n]
Struct(d,p) == Value(1,d,p,0,0)
List(e,n,d,p) == Value(2,d,p,e,n)
Shape(i) == CASE i=0 -> Value(0,<<>>,<<>>,0,0)
 [] i=1 -> Struct(<<>>,<<>>)
 [] i=2 -> Struct(<<0,0>>,<<0,0>>)
 [] i=3 -> Struct(<<42>>,<<>>)
 [] i=4 -> Struct(<<42,0>>,<<0,0>>)
 [] i=5 -> Struct(<<43>>,<<>>)
 [] i=6 -> Struct(<<>>,<<24>>)
 [] i=7 -> Struct(<<>>,<<25>>)
 [] i=8 -> Struct(<<1>>,<<24>>)
 [] i=9 -> Struct(<<>>,<<24,0>>)
 [] i=10 -> Struct(<<>>,<<24,17>>)
 [] i=11 -> Struct(<<>>,<<25,18>>)
 [] i=12 -> Struct(<<>>,<<17,24>>)
 [] i\in 13..15 -> List(0,i-13,<<>>,<<>>)
 [] i\in 16..17 -> List(2,1,<<1>>,<<>>)
 [] i=18 -> List(2,1,<<2>>,<<>>)
 [] i=19 -> List(3,1,<<1>>,<<>>)
 [] i=20 -> List(1,3,<<253>>,<<>>)
 [] i=21 -> List(1,3,<<5>>,<<>>)
 [] i=22 -> List(1,3,<<4>>,<<>>)
 [] i=23 -> List(1,4,<<5>>,<<>>)
 [] i=24 -> Value(3,<<0>>,<<>>,0,0)
 [] i=25 -> Value(3,<<99>>,<<>>,0,0)
 [] i=26 -> List(6,2,<<>>,<<24,17>>)
 [] i=27 -> List(6,2,<<>>,<<25,17>>)
 [] i=28 -> List(6,2,<<>>,<<25,18>>)
 [] i=29 -> List(7,2,<<>>,<<40,41>>)
 [] i=30 -> List(7,2,<<>>,<<42,43>>)
 [] i=31 -> List(7,2,<<>>,<<47,44>>)
 [] i=32 -> List(7,0,<<>>,<<>>)
 [] i=33 -> List(6,0,<<>>,<<>>)
 [] i=34 -> List(7,1,<<>>,<<45>>)
 [] i=35 -> List(6,1,<<>>,<<46>>)
 [] i=40 -> Struct(<<42>>,<<24>>)
 [] i=41 -> Struct(<<42>>,<<17>>)
 [] i=42 -> Struct(<<42,0>>,<<25,0>>)
 [] i=43 -> Struct(<<42,0>>,<<17,0>>)
 [] i=44 -> Struct(<<43>>,<<17>>)
 [] i=45 -> Struct(<<>>,<<46>>)
 [] i=46 -> Value(3,<<1>>,<<>>,0,0)
 [] i=47 -> Struct(<<42>>,<<25>>)
Trim(s) == LET ends == {j\in 1..Len(s):s[j]#0} \cup {0}
               last == CHOOSE j\in ends: \A k\in ends:j>=k
           IN SubSeq(s,1,last)
Data(v,f) == IF v.kind=1 /\ f#"comparePadding" THEN Trim(v.data)
             ELSE IF v.kind=2 /\ v.encoding=1 /\ v.count%8#0 /\ f#"bitPadding"
                  THEN <<v.data[1] % (2^(v.count%8))>> ELSE v.data
Ptrs(v,f) == IF v.kind=1 /\ f#"compareNullFields" THEN Trim(v.ptrs) ELSE v.ptrs
RECURSIVE Fold(_,_)
Fold(s,f) == IF Len(s)=0 THEN 1
             ELSE IF Head(s)=2 THEN 2
             ELSE IF Head(s)=3 /\ f="stopAtUnknown" THEN 3
             ELSE LET rest == Fold(Tail(s),f)
                  IN IF rest=2 THEN 2
                     ELSE IF (Head(s)=3 \/ rest=3) /\ f#"forgetUnknown" THEN 3 ELSE 1
RECURSIVE Compare(_,_,_)
Compare(a,b,f) ==
 LET l==Shape(a) r==Shape(b) lp==Ptrs(l,f) rp==Ptrs(r,f)
 IN IF l.kind#r.kind THEN 2
    ELSE IF l.kind=0 THEN 1
    ELSE IF l.kind=3 THEN IF f="capIdentity" THEN IF l.data=r.data THEN 1 ELSE 2 ELSE 3
    ELSE IF l.kind=2 /\ ((l.encoding#r.encoding /\ f#"ignoreEncoding")
                       \/ (l.count#r.count /\ f#"ignoreLength")) THEN 2
    ELSE IF Data(l,f)#Data(r,f) \/ Len(lp)#Len(rp) THEN 2
    ELSE Fold([j\in 1..Len(lp)|->Compare(lp[j],rp[j],f)],f)
Init == /\ phase=0 /\ left=0 /\ right=0 /\ result=0 /\ event=0
SelectLeft(i) == /\ phase=0 /\ phase'=1 /\ left'=i /\ event'=1+i
                 /\ UNCHANGED <<right,result>>
SelectRight(i) == /\ phase=1 /\ phase'=2 /\ right'=i /\ event'=101+i
                  /\ UNCHANGED <<left,result>>
Check(reverse) == /\ phase=2 /\ phase'=3 /\ event'=201+reverse
                  /\ result'=IF reverse=0 THEN Compare(left,right,Fault) ELSE Compare(right,left,Fault)
                  /\ UNCHANGED <<left,right>>
Next == (\E i\in 0..35:SelectLeft(i) \/ SelectRight(i)) \/ Check(0) \/ Check(1)
Spec == Init /\ [][Next]_vars
TypeOK == /\ phase\in 0..3 /\ left\in 0..35 /\ right\in 0..35
          /\ result\in 0..3 /\ event\in 0..202
StructuralContract == phase#3 \/ result=Compare(left,right,"none")
=============================================================================
