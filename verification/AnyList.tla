------------------------------ MODULE AnyList ------------------------------
EXTENDS Naturals
CONSTANT Fault
\* Twelve layouts: seven non-struct encodings and inline (data,pointers)
\* (0,0), (1,0), (0,1), (1,1), (2,2). Zero to two elements, four actions.
\* First/last data byte (bit for packed lists) and pointer slot writes, copy,
\* clear, raw byte inspection. Capability slots remain callable after copying.
VARIABLES kind,n,a,b,x,y,allocated,copied,snapshot,seenKind,seenCount,
          seenValue,seenCopy,seenWords,seenCaps,raw,rawBytes,projected,bitCast,bitRead,steps,event
vars == <<kind,n,a,b,x,y,allocated,copied,snapshot,seenKind,seenCount,
          seenValue,seenCopy,seenWords,seenCaps,raw,rawBytes,projected,bitCast,bitRead,steps,event>>
Data(k) == CASE k=1 -> 1 [] k=2 -> 8 [] k=3 -> 16 [] k=4 -> 32
               [] k\in {5,8,10} -> 64 [] k=11 -> 128 [] OTHER -> 0
Pointers(k) == CASE k\in {6,9,10} -> 1 [] k=11 -> 2 [] OTHER -> 0
Encoding(k) == IF k>=7 THEN 7 ELSE k
SameData(k,c) == c=1 /\ k\in {1,2}
SamePointer(k,c) == c=1 /\ Pointers(k)=1
Extra(k,c,xx,yy,v) == IF c=0 THEN 0 ELSE
   (IF xx=v THEN 1 ELSE 0)+(IF yy=v /\ ~SamePointer(k,c) THEN 1 ELSE 0)
Words(k,c,xx,yy) == (c*(Data(k)+64*Pointers(k))+63)\div 64
                    +(IF k>=7 THEN 1 ELSE 0)+Extra(k,c,xx,yy,1)
Value(aa,bb,xx,yy) == 1000*aa+100*bb+10*xx+yy
Snapshot(k,c,aa,bb,xx,yy) == 10000000+1000000*k+100000*c+10000*aa+1000*bb+10*xx+yy
Init == /\ kind=0 /\ n=0 /\ a=0 /\ b=0 /\ x=0 /\ y=0 /\ allocated=0
        /\ copied=0 /\ snapshot=0 /\ seenKind=0 /\ seenCount=0 /\ seenValue=0
        /\ seenCopy=0 /\ seenWords=0 /\ seenCaps=0 /\ raw=0 /\ rawBytes=0
        /\ projected=0 /\ bitCast=0 /\ bitRead=0 /\ steps=0 /\ event=0
Allocate(k,c) == /\ steps=0 /\ kind'=k /\ n'=c /\ allocated'=1 /\ event'=1+3*k+c
                 /\ UNCHANGED <<a,b,x,y,copied,snapshot>>
Write(first) ==
 /\ allocated=1 /\ n>0 /\ Data(kind)>0 /\ event'=IF first THEN 100 ELSE 101
 /\ a'=(IF first \/ SameData(kind,n) THEN 1 ELSE a)
 /\ b'=(IF ~first \/ SameData(kind,n) THEN 1 ELSE b)
 /\ UNCHANGED <<kind,n,x,y,allocated,copied,snapshot>>
Pointer(first,v) ==
 /\ allocated=1 /\ n>0 /\ Pointers(kind)>0 /\ event'=200+(IF first THEN 0 ELSE 3)+v
 /\ x'=(IF first \/ SamePointer(kind,n) THEN v ELSE x)
 /\ y'=(IF ~first \/ SamePointer(kind,n) THEN v ELSE y)
 /\ UNCHANGED <<kind,n,a,b,allocated,copied,snapshot>>
Copy == /\ allocated=1 /\ event'=300
        /\ snapshot'=Snapshot(kind,n,a,b,x,y)
        /\ copied'=Snapshot(kind,n,a,b,IF Fault="dropCaps" /\ x=2 THEN 0 ELSE x,
                                           IF Fault="dropCaps" /\ y=2 THEN 0 ELSE y)
        /\ UNCHANGED <<kind,n,a,b,x,y,allocated>>
Clear == /\ allocated=1 /\ event'=301 /\ allocated'=0
         /\ kind'=0 /\ n'=0 /\ a'=0 /\ b'=0 /\ x'=0 /\ y'=0
         /\ UNCHANGED <<copied,snapshot>>
Raw == /\ allocated=1 /\ event'=302
       /\ UNCHANGED <<kind,n,a,b,x,y,allocated,copied,snapshot>>
\* Set through a typed pointer-list builder, convert to a reader, erase its
\* schema and observe the first pointer. The element's data prefix stays intact.
Project == /\ allocated=1 /\ n>0 /\ Pointers(kind)>0 /\ event'=303
           /\ x'=1 /\ y'=(IF SamePointer(kind,n) THEN 1 ELSE y)
           /\ UNCHANGED <<kind,n,a,b,allocated,copied,snapshot>>
BitView == /\ allocated=1 /\ event'=304
           /\ UNCHANGED <<kind,n,a,b,x,y,allocated,copied,snapshot>>
Observe ==
 /\ seenKind'=Encoding(kind')
 /\ seenCount'=IF Fault="truncateCount" /\ n'=2 THEN 1 ELSE n'
 /\ seenValue'=Value(a',IF Fault="wrongStride" THEN a' ELSE b',x',y')
 /\ seenCopy'=IF Fault="aliasCopy" /\ copied'#0 THEN Snapshot(kind',n',a',b',x',y') ELSE copied'
 /\ seenWords'=Words(kind',n',x',y')-(IF Fault="dropTag" /\ kind'>=7 THEN 1 ELSE 0)
 /\ seenCaps'=IF Fault="hideCaps" THEN 0 ELSE Extra(kind',n',x',y',2)
 /\ raw'=IF event'#302 THEN 0 ELSE IF Pointers(kind)=0 \/ Fault="exposePointers" THEN 1 ELSE 2
 /\ rawBytes'=IF raw'#1 THEN 0 ELSE
       (n*Data(kind)+(IF Fault="truncateBits" THEN 0 ELSE 7))\div 8
 /\ projected'=IF event'#303 \/ (Fault="doubleOffset" /\ Data(kind)>0) THEN 0 ELSE 1
 /\ bitCast'=IF event'#304 THEN 0 ELSE IF kind=1 \/ Fault="allowNonBit" THEN 1 ELSE 2
 \* Ordinary typed C++ readers accept larger primitive encodings as bits,
 \* while writable getters require packed bits. The new checked AnyList casts
 \* are stricter than those ordinary readers; native tests cover that boundary.
 /\ bitRead'=IF event'#304 THEN 0 ELSE IF kind\in 1..5 THEN 1 ELSE 2
Next == /\ steps<4 /\ steps'=steps+1
        /\ ((\E k\in 0..11,c\in 0..2: Allocate(k,c)) \/ Write(TRUE) \/ Write(FALSE)
            \/ (\E first\in BOOLEAN,v\in 0..2: Pointer(first,v)) \/ Copy \/ Clear \/ Raw \/ Project \/ BitView)
        /\ Observe
Spec == Init /\ [][Next]_vars
TypeOK == /\ kind\in 0..11 /\ n\in 0..2 /\ a\in 0..1 /\ b\in 0..1
          /\ x\in 0..2 /\ y\in 0..2 /\ allocated\in 0..1 /\ steps\in 0..4
Layout == seenKind=Encoding(kind) /\ seenCount=n /\ seenValue=Value(a,b,x,y)
Size == seenWords=Words(kind,n,x,y) /\ seenCaps=Extra(kind,n,x,y,2)
IndependentCopy == seenCopy=snapshot
ProjectionContract == event=303 => projected=1
BitContract == event=304 => /\ bitCast=(IF kind=1 THEN 1 ELSE 2)
                           /\ bitRead=(IF kind\in 1..5 THEN 1 ELSE 2)
RawContract == event=302 => /\ raw=(IF Pointers(kind)=0 THEN 1 ELSE 2)
                            /\ rawBytes=(IF Pointers(kind)=0 THEN (n*Data(kind)+7)\div 8 ELSE 0)
=============================================================================
