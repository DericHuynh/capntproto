--------------------------- MODULE RpcSocketWindow ---------------------------
EXTENDS Naturals
CONSTANT Fault, MaxSteps
\* Two independently accounted streams share a send-buffer query/cache.
\* Unit = 16 KiB; hints 0/1/2/3 mean unavailable/zero/one/four units.
\* Three sends on stream one, two on stream two; the first call on either may
\* acknowledge or fail. Up to two hint changes and MaxSteps total actions.
\* Query timing matters: C++ skips it when in-flight <= largest message and
\* after failure. Changes alone do not wake credit. Actual socket IO is outside
\* this model. expectedQueries/expectedWindow specify the query contract.
VARIABLES sent1,sent2,ack1,ack2,p11,p12,p13,p21,p22,hint,changes,disabled,queries,
          expectedQueries,used,expectedWindow,sampled,prior1,prior2,prior3,step,event
vars == <<sent1,sent2,ack1,ack2,p11,p12,p13,p21,p22,hint,changes,disabled,queries,
          expectedQueries,used,expectedWindow,sampled,prior1,prior2,prior3,step,event>>
HintSize == IF hint=1 THEN 0 ELSE IF hint=2 THEN 1 ELSE 4
Window == IF disabled=1 THEN 4 ELSE HintSize
Init == /\ sent1=0 /\ sent2=0 /\ ack1=0 /\ ack2=0
        /\ p11=0 /\ p12=0 /\ p13=0 /\ p21=0 /\ p22=0
        /\ hint=2 /\ changes=0 /\ disabled=0 /\ queries=0 /\ expectedQueries=0
        /\ used=0 /\ expectedWindow=0 /\ sampled=0
        /\ prior1=0 /\ prior2=0 /\ prior3=0 /\ step=0 /\ event=0
Sample(active,isAck,secondStream) ==
    LET query == (active \/ Fault="eagerQuery") /\
                 (disabled=0 \/ Fault="retryUnavailable" \/
                   (Fault="perStreamCache" /\ secondStream)) /\
                  ~(Fault="skipAckQuery" /\ isAck)
    IN /\ queries'=queries + (IF query THEN 1 ELSE 0)
       /\ expectedQueries'=expectedQueries + (IF active /\ disabled=0 THEN 1 ELSE 0)
       /\ disabled'=IF disabled=1 \/ (active /\ hint=0) THEN 1 ELSE 0
       /\ expectedWindow'=IF active THEN Window ELSE 0
       /\ used'=IF ~active THEN 0 ELSE IF Fault="staleWindow" THEN 1
                ELSE IF Fault="zeroUnavailable" /\ hint=1 THEN 4 ELSE Window
       /\ sampled'=IF active THEN 1 ELSE 0
Ready(bytes,w) == bytes<=1 \/ bytes<1+w
Send(s) ==
    /\ IF s=1 THEN sent1<3 ELSE sent2<2
    /\ LET sent == IF s=1 THEN sent1 ELSE sent2
           ack == IF s=1 THEN ack1 ELSE ack2
           bytes == sent+1-(IF ack=1 THEN 1 ELSE 0)
       IN /\ Sample(ack#2 /\ bytes>1,FALSE,s=2)
          /\ LET credit == IF ack=2 THEN 3 ELSE IF Ready(bytes,used') THEN 1 ELSE 2
             IN /\ p12'=IF s=1 /\ sent1=1 THEN credit ELSE p12
                /\ p13'=IF s=1 /\ sent1=2 THEN credit ELSE p13
                /\ p22'=IF s=2 /\ sent2=1 THEN credit ELSE p22
    /\ sent1'=IF s=1 THEN sent1+1 ELSE sent1
    /\ sent2'=IF s=2 THEN sent2+1 ELSE sent2
    /\ p11'=IF s=1 /\ sent1=0 THEN 1 ELSE p11
    /\ p21'=IF s=2 /\ sent2=0 THEN 1 ELSE p21
    /\ event'=s /\ step'=step+1
    /\ UNCHANGED <<ack1,ack2,hint,changes,prior1,prior2,prior3>>
Ack(s,error) ==
    /\ IF s=1 THEN sent1>0 /\ ack1=0 ELSE sent2>0 /\ ack2=0
    /\ Sample(~error /\ (IF s=1 THEN sent1 ELSE sent2)>2,TRUE,s=2)
    /\ ack1'=IF s=1 THEN (IF error THEN 2 ELSE 1) ELSE ack1
    /\ ack2'=IF s=2 THEN (IF error THEN 2 ELSE 1) ELSE ack2
    /\ LET Release(p,bytes) == IF p#2 THEN p ELSE IF error THEN 3
                              ELSE IF Ready(bytes,used') THEN 1 ELSE 2
       IN /\ p12'=IF s=1 THEN Release(p12,sent1-1) ELSE p12
          /\ p13'=IF s=1 THEN Release(p13,sent1-1) ELSE p13
          /\ p22'=IF s=2 THEN Release(p22,sent2-1) ELSE p22
    /\ event'=2+s*2+(IF error THEN 1 ELSE 0) /\ step'=step+1
    /\ UNCHANGED <<sent1,sent2,p11,p21,hint,changes,prior1,prior2,prior3>>
Change(h) ==
    /\ changes<2 /\ h#hint /\ hint'=h /\ changes'=changes+1
    /\ prior1'=p12 /\ prior2'=p13 /\ prior3'=p22
    /\ LET Wake(p) == IF Fault="wakeOnResize" /\ p=2 /\ h=3 THEN 1 ELSE p
       IN /\ p12'=Wake(p12) /\ p13'=Wake(p13) /\ p22'=Wake(p22)
    /\ sampled'=0 /\ event'=10+h /\ step'=step+1
    /\ UNCHANGED <<sent1,sent2,ack1,ack2,p11,p21,disabled,queries,
                    expectedQueries,used,expectedWindow>>
Next == /\ step<MaxSteps
        /\ (Send(1) \/ Send(2) \/ (\E s\in 1..2,e\in BOOLEAN: Ack(s,e))
             \/ (\E h\in 0..3: Change(h)))
Spec == Init /\ [][Next]_vars
TypeOK == /\ sent1\in 0..3 /\ sent2\in 0..2 /\ ack1\in 0..2 /\ ack2\in 0..2
          /\ p11\in 0..1 /\ p12\in 0..3 /\ p13\in 0..3 /\ p21\in 0..1 /\ p22\in 0..3
          /\ hint\in 0..3 /\ changes\in 0..2 /\ disabled\in 0..1 /\ sampled\in 0..1
          /\ queries\in 0..MaxSteps /\ expectedQueries\in 0..MaxSteps
          /\ used\in {0,1,4} /\ expectedWindow\in {0,1,4}
          /\ prior1\in 0..3 /\ prior2\in 0..3 /\ prior3\in 0..3
          /\ step\in 0..MaxSteps /\ event\in 0..13
QueryContract == queries=expectedQueries
SampledWindow == sampled=1 => used=expectedWindow
ResizePreservesCredit == event>=10 => <<p12,p13,p22>> = <<prior1,prior2,prior3>>
FailureReleasesCredit == /\ (ack1=2 => p12#2 /\ p13#2)
                        /\ (ack2=2 => p22#2)
=============================================================================
