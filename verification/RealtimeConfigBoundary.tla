----------------------- MODULE RealtimeConfigBoundary -----------------------
EXTENDS Naturals
CONSTANT Fault
\* Import local UTF-8 or untrusted wire limits. Then probe admission, a retained
\* duplicate receipt and publication. This is finite safety, not clock trust,
\* transport delivery, full receipt scheduling or an allocator-memory proof.
\* Tuple: domain byte length, UTF-8, skew, keys, capacity, sequence, payload, waiters.
VARIABLES domain,utf8,skew,keys,capacity,limit,payload,waiters,source,event,
          accepted,sequence,key,size,result,expected,outcome,held,published
vars == <<domain,utf8,skew,keys,capacity,limit,payload,waiters,source,event,
          accepted,sequence,key,size,result,expected,outcome,held,published>>
Small == {<<1,1,s,k,c,q,p,w>>: s\in {0,2},k,c,q,p,w\in 0..2}
Boundary == {<<0,1,0,1,1,1,1,1>>, <<128,1,0,1,1,1,1,1>>,
             <<129,1,0,1,1,1,1,1>>, <<1,0,0,1,1,1,1,1>>,
             <<1,1,0,1025,1,1,1,1>>, <<1,1,0,1,1,65537,1,1>>,
             <<1,1,0,1,1,1,1048577,1>>, <<1,1,0,1,1,1,1,65537>>,
             <<1,1,0,32,32,65536,1048576,65536>>,
             <<1,1,0,33,32,65536,1048576,65536>>,
             <<1,1,0,1024,1024,1,32768,1>>,
             <<1,1,0,1024,1024,1,32769,1>>}
\* Division keeps the actual 64 MiB boundary within TLC's 32-bit arithmetic.
Budget(k,c,p) == IF p=0 THEN TRUE ELSE k+c <= 67108864\div p
Valid(d,u,k,c,q,p,w) == d\in 1..128 /\ u=1 /\ k\in 1..1024
                       /\ c\in 1..k /\ q\in 1..65536
                       /\ p\in 1..1048576 /\ w\in 1..65536 /\ Budget(k,c,p)
Init == /\ domain=0 /\ utf8=0 /\ skew=0 /\ keys=0 /\ capacity=0
        /\ limit=0 /\ payload=0 /\ waiters=0 /\ source=0 /\ event=0
        /\ accepted=0 /\ sequence=0 /\ key=0 /\ size=0 /\ result=0
        /\ expected=0 /\ outcome=0 /\ held=0 /\ published=0
Import(v,s) ==
    /\ event=0 /\ (s=2 \/ v[2]=1)
    /\ domain'=v[1] /\ utf8'=v[2] /\ skew'=v[3] /\ keys'=v[4]
    /\ capacity'=v[5] /\ limit'=v[6] /\ payload'=v[7] /\ waiters'=v[8]
    /\ LET d==v[1] u==v[2] k==v[4] c==v[5] q==v[6] p==v[7] w==v[8]
           allow == (d\in 1..128 \/ Fault="domain") /\ (u=1 \/ Fault="utf8")
                    /\ (k\in 1..1024 \/ Fault="keys")
                    /\ (c\in 1..k \/ Fault="capacity")
                    /\ (q\in 1..65536 \/ Fault="sequence")
                    /\ (p\in 1..1048576 \/ Fault="payload")
                    /\ (w\in 1..65536 \/ Fault="waiters")
                    /\ (Budget(k,c,p) \/ Fault="budget")
       IN /\ result'=(IF allow THEN 1 ELSE 0)
          /\ expected'=(IF Valid(d,u,k,c,q,p,w) THEN 1 ELSE 0)
    /\ accepted'=result' /\ source'=s /\ event'=1
    /\ UNCHANGED <<sequence,key,size,outcome,held,published>>
Bounds(s,k,z) == s\in 1..limit /\ k<keys /\ z<=payload
Offer(s,k,z) ==
    /\ event=1 /\ accepted=1 /\ keys<=2 /\ limit<=2 /\ payload<=2
    /\ sequence'=s /\ key'=k /\ size'=z /\ event'=2
    /\ result'=(IF Bounds(s,k,z) \/ Fault="admission" THEN 1 ELSE 0)
    /\ expected'=(IF Bounds(s,k,z) THEN 1 ELSE 0)
    /\ outcome'=(IF result'=0 THEN 0 ELSE IF skew<2 \/ Fault="deadline" THEN 1 ELSE 3)
    /\ held'=(IF outcome'=1 THEN 1 ELSE 0)
    /\ UNCHANGED <<domain,utf8,skew,keys,capacity,limit,payload,waiters,source,accepted,published>>
Duplicate ==
    /\ event=2 /\ event'=3
    /\ expected'=(IF outcome=0 THEN 0 ELSE IF outcome=1 /\ held=waiters THEN 2 ELSE 1)
    /\ result'=(IF outcome=0 THEN 0 ELSE IF outcome=1 /\ held=waiters /\ Fault#"quota" THEN 2 ELSE 1)
    /\ held'=(IF result'=1 /\ outcome=1 THEN held+1 ELSE held)
    /\ UNCHANGED <<domain,utf8,skew,keys,capacity,limit,payload,waiters,source,accepted,
                    sequence,key,size,outcome,published>>
Apply ==
    /\ event\in {2,3} /\ event'=4
    /\ result'=(IF outcome=0 THEN 0 ELSE 1) /\ expected'=result'
    /\ outcome'=(IF outcome=1 THEN 2 ELSE outcome)
    /\ published'=(IF outcome'=2 THEN 1 ELSE 0) /\ held'=0
    /\ UNCHANGED <<domain,utf8,skew,keys,capacity,limit,payload,waiters,source,accepted,sequence,key,size>>
Next == (\E v\in Small\cup Boundary,s\in 1..2: Import(v,s))
        \/ (\E s,z\in 0..3,k\in 0..2: Offer(s,k,z)) \/ Duplicate \/ Apply
Spec == Init /\ [][Next]_vars
TypeOK == /\ domain\in 0..129 /\ utf8\in 0..1 /\ skew\in {0,2}
          /\ keys\in 0..1025 /\ capacity\in 0..1024 /\ limit\in 0..65537
          /\ payload\in 0..1048577 /\ waiters\in 0..65537 /\ source\in 0..2
          /\ event\in 0..4 /\ accepted\in 0..1 /\ sequence\in 0..3
          /\ key\in 0..2 /\ size\in 0..3 /\ result\in 0..2 /\ expected\in 0..2
          /\ outcome\in 0..3 /\ held\in 0..2 /\ published\in 0..1
ExpectedImport == event=1 => result=expected
ExpectedOperation == event>1 => result=expected
ValidReceiver == accepted=1 => Valid(domain,utf8,keys,capacity,limit,payload,waiters)
ResourceSafety == held<=waiters /\ (outcome#0 => Bounds(sequence,key,size))
DeadlineSafety == outcome\in {1,2} => skew<2
Publication == (published=1 <=> outcome=2) /\ (outcome#1 => held=0)
=============================================================================
