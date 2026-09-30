------------------------- MODULE BulkConfigBoundary -------------------------
EXTENDS Naturals, FiniteSets
CONSTANT Fault
\* Three import paths, then at most three receiver calls. Large boundary cases
\* are validated without allocating their payload. Status: absent, receiving,
\* complete, canceled, failed = 0..4; event: import, write, done, cancel = 1..4.
VARIABLES length,chunk,window,count,source,event,sequence,size,accepted,status,
          bytes,chunks,staged,published,failed,steps,result,expected
vars == <<length,chunk,window,count,source,event,sequence,size,accepted,status,
          bytes,chunks,staged,published,failed,steps,result,expected>>
Small == {<<l,c,w,n>>: l\in 0..3,c\in 0..2,w\in 0..2,n\in 0..2}
Boundary == {<<67108864,1048576,16777216,65536>>,
             <<67108865,1048576,16777216,65536>>,
             <<0,1048577,16777216,65536>>,
             <<0,1,16777217,1>>, <<0,1,1,65537>>,
             <<67108864,1048576,1048576,64>>,
             <<67108864,1048576,1048576,63>>}
\* Ceiling division avoids TLC's signed 32-bit multiplication limit for the
\* full-size boundary cases; Rust checks the equivalent product in u64.
Capacity(l,c,n) == IF c=0 THEN l=0
                  ELSE l\div c + (IF l%c=0 THEN 0 ELSE 1) <= n
Valid(l,c,w,n) == l<=67108864 /\ c\in 1..1048576 /\ w>=c /\ w<=16777216
                 /\ n\in 1..65536 /\ Capacity(l,c,n)
Init == /\ length=0 /\ chunk=0 /\ window=0 /\ count=0 /\ source=0
        /\ event=0 /\ sequence=0 /\ size=0 /\ accepted=0 /\ status=0
        /\ bytes=0 /\ chunks=0 /\ staged=0 /\ published=0 /\ failed=0
        /\ steps=0 /\ result=0 /\ expected=0
Import(v,s) ==
    /\ event=0
    /\ length'=v[1] /\ chunk'=v[2] /\ window'=v[3] /\ count'=v[4]
    /\ LET l==v[1] c==v[2] w==v[3] n==v[4]
           allow == (l<=67108864 \/ Fault="length")
                    /\ (c\in 1..1048576 \/ Fault="chunk")
                    /\ ((w>=c /\ w<=16777216) \/ Fault="window")
                    /\ (n\in 1..65536 \/ Fault="count")
                    /\ (Capacity(l,c,n) \/ Fault="capacity")
       IN /\ accepted'=(IF allow THEN 1 ELSE 0)
          /\ expected'=(IF Valid(l,c,w,n) THEN 1 ELSE 0)
    /\ source'=s /\ event'=1 /\ status'=accepted' /\ result'=accepted'
    /\ UNCHANGED <<sequence,size,bytes,chunks,staged,published,failed,steps>>
Ready == accepted=1 /\ length<=3 /\ chunk<=2 /\ count<=2 /\ steps<3
Write(s,z) ==
    /\ Ready /\ event'=2 /\ sequence'=s /\ size'=z /\ steps'=steps+1
    /\ LET normal == status=1 /\ s=chunks+1 /\ s<=count /\ z>0
                     /\ z<=chunk /\ bytes+z<=length
           allow == status=1 /\ s=chunks+1 /\ s<=count /\ z>0
                    /\ (z<=chunk \/ Fault="write") /\ bytes+z<=length
       IN /\ result'=(IF allow THEN 1 ELSE 0)
          /\ expected'=(IF normal THEN 1 ELSE 0)
          /\ bytes'=(IF allow THEN bytes+z ELSE bytes)
          /\ chunks'=(IF allow THEN chunks+1 ELSE chunks)
          /\ status'=(IF status=1 /\ ~allow THEN 4 ELSE status)
    /\ staged'=(IF status'=1 THEN bytes' ELSE 0)
    /\ failed'=(IF status'=4 THEN 1 ELSE failed)
    /\ UNCHANGED <<length,chunk,window,count,source,accepted,published>>
Done ==
    /\ Ready /\ event'=3 /\ sequence'=0 /\ size'=0 /\ steps'=steps+1
    /\ LET normal == status=2 \/ (status=1 /\ bytes=length)
           allow == status=2 \/ (status=1 /\ (bytes=length \/ Fault="publish"))
       IN /\ result'=(IF allow THEN 1 ELSE 0)
          /\ expected'=(IF normal THEN 1 ELSE 0)
          /\ status'=(IF allow THEN 2 ELSE IF status=1 THEN 4 ELSE status)
    /\ staged'=0 /\ published'=(IF status'=2 THEN 1 ELSE published)
    /\ failed'=(IF status'=4 THEN 1 ELSE failed)
    /\ UNCHANGED <<length,chunk,window,count,source,accepted,bytes,chunks>>
Cancel ==
    /\ Ready /\ event'=4 /\ sequence'=0 /\ size'=0 /\ steps'=steps+1
    /\ status'=(IF status=2 THEN 2 ELSE 3) /\ staged'=0
    /\ result'=1 /\ expected'=1
    /\ UNCHANGED <<length,chunk,window,count,source,accepted,bytes,chunks,published,failed>>
Next == (\E v\in Small\cup Boundary,s\in 1..3: Import(v,s))
        \/ (\E s,z\in 0..3: Write(s,z)) \/ Done \/ Cancel
Spec == Init /\ [][Next]_vars
TypeOK == /\ length\in 0..67108865 /\ chunk\in 0..1048577 /\ window\in 0..16777217
          /\ count\in 0..65537 /\ source\in 0..3 /\ event\in 0..4
          /\ sequence\in 0..3 /\ size\in 0..3 /\ accepted\in 0..1 /\ status\in 0..4
          /\ bytes\in 0..3 /\ chunks\in 0..3 /\ staged\in 0..3 /\ published\in 0..1
          /\ failed\in 0..1 /\ steps\in 0..3 /\ result\in 0..1 /\ expected\in 0..1
ExpectedImport == event=1 => result=expected
ExpectedOperation == event>1 => result=expected
ValidReceiver == accepted=1 => Valid(length,chunk,window,count)
TransferBounds == bytes<=length /\ chunks<=count /\ bytes<=chunks*chunk
Publication == published=1 => (status=2 /\ bytes=length /\ failed=0)
Staging == (status=1 => staged=bytes) /\ (status#1 => staged=0)
=============================================================================
