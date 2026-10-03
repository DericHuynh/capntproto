----------------------- MODULE RpcNativeArbitration -----------------------
EXTENDS Naturals, FiniteSets, TLC
CONSTANTS Count, Bug
VARIABLES l,f,d,a,c,sent,wl,wf,firstl,firstf,pl,pf,cl,cf,dl,df,badEpoch,badClosed,event,item,
          l1,l2,f1,f2,d1,d2,a1,a2,c1,c2,s1,s2
vars == <<l,f,d,a,c,sent,wl,wf,firstl,firstf,pl,pf,cl,cf,dl,df,badEpoch,badClosed,event,item,
          l1,l2,f1,f2,d1,d2,a1,a2,c1,c2,s1,s2>>
Ids == 1..Count
\* Both candidates already authenticated and exchanged valid epoch hellos.
\* Leader: hello, await ack, write commit, ready, rejected, dropped.
\* Follower: unused, await decision, await commit, ready, rejected, dropped.
Init == /\ l=[i\in Ids|->0] /\ f=[i\in Ids|->1]
        /\ d=l /\ a=l /\ c=l /\ sent=[i\in Ids|->FALSE]
        /\ wl=0 /\ wf=0 /\ firstl=0 /\ firstf=0 /\ pl=FALSE /\ pf=FALSE
        /\ cl=FALSE /\ cf=FALSE /\ dl=0 /\ df=0 /\ badEpoch=FALSE /\ badClosed=FALSE
        /\ event=0 /\ item=0 /\ l1=0 /\ l2=0 /\ f1=1 /\ f2=IF Count=2 THEN 1 ELSE 0
        /\ d1=0 /\ d2=0 /\ a1=0 /\ a2=0 /\ c1=0 /\ c2=0 /\ s1=FALSE /\ s2=FALSE
Select(i) == /\ l[i]=0 /\ event'=1 /\ item'=i
             /\ LET ok == ~cl /\ (wl=0 \/ Bug="doubleSelection")
                IN /\ l'=[l EXCEPT ![i]=IF ok THEN 1 ELSE 4]
                   /\ d'=[d EXCEPT ![i]=IF ok THEN 1 ELSE 2]
                   /\ wl'=IF ok THEN i ELSE wl
                   /\ firstl'=IF ok /\ firstl=0 THEN i ELSE firstl
             /\ UNCHANGED <<f,a,c,sent,wf,firstf,pl,pf,cl,cf,dl,df,badEpoch,badClosed>>
Decision(i) == /\ f[i]=1 /\ d[i]>0 /\ event'=2 /\ item'=i
               /\ LET ok == d[i]=1 /\ ~cf /\ wf=0
                  IN /\ f'=[f EXCEPT ![i]=IF ok THEN (IF Bug="skipCommit" THEN 3 ELSE 2) ELSE 4]
                     /\ a'=[a EXCEPT ![i]=IF d[i]=2 THEN 0 ELSE IF ok THEN 1 ELSE 2]
                     /\ wf'=IF ok THEN i ELSE wf
                     /\ firstf'=IF ok /\ firstf=0 THEN i ELSE firstf
                     /\ pf'=(pf \/ (ok /\ Bug="skipCommit"))
               /\ d'=[d EXCEPT ![i]=0]
               /\ UNCHANGED <<l,c,sent,wl,firstl,pl,cl,cf,dl,df,badEpoch,badClosed>>
Ack(i) == /\ l[i]=1 /\ a[i]>0 /\ event'=3 /\ item'=i
          /\ l'=[l EXCEPT ![i]=IF a[i]=1 THEN 2 ELSE 4]
          /\ cl'=(cl \/ (a[i]=2 /\ wl=i /\ ~pl))
          /\ a'=[a EXCEPT ![i]=0]
          /\ UNCHANGED <<f,d,c,sent,wl,wf,firstl,firstf,pl,pf,cf,dl,df,badEpoch,badClosed>>
Commit(i) == /\ (l[i]=2 \/ (Bug="skipAck" /\ l[i]=1)) /\ event'=4 /\ item'=i
             /\ l'=[l EXCEPT ![i]=IF ~cl \/ Bug="closedPublish" THEN 3 ELSE 5]
             /\ pl'=(pl \/ ~cl \/ Bug="closedPublish")
             /\ c'=[c EXCEPT ![i]=IF Bug="lostCommit" THEN 0 ELSE 1] /\ sent'=[sent EXCEPT ![i]=TRUE]
             /\ badClosed'=(badClosed \/ (cl /\ Bug="closedPublish"))
             /\ UNCHANGED <<f,d,a,wl,wf,firstl,firstf,pf,cl,cf,dl,df,badEpoch>>
Confirm(i) == /\ f[i]=2 /\ c[i]=1 /\ event'=5 /\ item'=i
              /\ f'=[f EXCEPT ![i]=IF ~cf \/ Bug="closedPublish" THEN 3 ELSE 5]
              /\ pf'=(pf \/ ~cf \/ Bug="closedPublish") /\ c'=[c EXCEPT ![i]=0]
              /\ badClosed'=(badClosed \/ (cf /\ Bug="closedPublish"))
              /\ UNCHANGED <<l,d,a,sent,wl,wf,firstl,firstf,pl,cl,cf,dl,df,badEpoch>>
DropL(i) == /\ l[i]\in {0,1,2,4} /\ event'=6 /\ item'=i /\ l'=[l EXCEPT ![i]=5]
            /\ cl'=(cl \/ (wl=i /\ ~pl /\ Bug#"retryWinner"))
            /\ wl'=IF wl=i /\ ~pl /\ Bug="retryWinner" THEN 0 ELSE wl
            /\ UNCHANGED <<f,d,a,c,sent,wf,firstl,firstf,pl,pf,cf,dl,df,badEpoch,badClosed>>
DropF(i) == /\ f[i]\in {1,2,4} /\ event'=7 /\ item'=i /\ f'=[f EXCEPT ![i]=5]
            /\ cf'=(cf \/ (wf=i /\ ~pf))
            /\ UNCHANGED <<l,d,a,c,sent,wl,wf,firstl,firstf,pl,pf,cl,dl,df,badEpoch,badClosed>>
CloseL == /\ ~cl /\ event'=8 /\ item'=0 /\ cl'=TRUE
          /\ UNCHANGED <<l,f,d,a,c,sent,wl,wf,firstl,firstf,pl,pf,cf,dl,df,badEpoch,badClosed>>
CloseF == /\ ~cf /\ event'=9 /\ item'=0 /\ cf'=TRUE
          /\ UNCHANGED <<l,f,d,a,c,sent,wl,wf,firstl,firstf,pl,pf,cl,dl,df,badEpoch,badClosed>>
DataL(i) == /\ dl=0 /\ ~cl /\ ((pl /\ l[i]=3) \/ Bug="earlyData")
            /\ dl'=i /\ event'=10 /\ item'=i
            /\ UNCHANGED <<l,f,d,a,c,sent,wl,wf,firstl,firstf,pl,pf,cl,cf,df,badEpoch,badClosed>>
DataF(i) == /\ df=0 /\ ~cf /\ ((pf /\ f[i]=3) \/ Bug="earlyData")
            /\ df'=i /\ event'=11 /\ item'=i
            /\ UNCHANGED <<l,f,d,a,c,sent,wl,wf,firstl,firstf,pl,pf,cl,cf,dl,badEpoch,badClosed>>
BadEpoch(i) == /\ f[i]=2 /\ event'=12 /\ item'=i
               /\ f'=[f EXCEPT ![i]=IF Bug="staleEpoch" THEN 3 ELSE 5]
               /\ pf'=(pf \/ Bug="staleEpoch") /\ cf'=(cf \/ Bug#"staleEpoch")
               /\ badEpoch'=(badEpoch \/ Bug="staleEpoch")
               /\ UNCHANGED <<l,d,a,c,sent,wl,wf,firstl,firstf,pl,cl,dl,df,badClosed>>
Project == /\ l1'=l'[1] /\ f1'=f'[1] /\ d1'=d'[1] /\ a1'=a'[1] /\ c1'=c'[1] /\ s1'=sent'[1]
           /\ l2'=(IF Count=2 THEN l'[2] ELSE 0) /\ f2'=(IF Count=2 THEN f'[2] ELSE 0)
           /\ d2'=(IF Count=2 THEN d'[2] ELSE 0) /\ a2'=(IF Count=2 THEN a'[2] ELSE 0)
           /\ c2'=(IF Count=2 THEN c'[2] ELSE 0) /\ s2'=(IF Count=2 THEN sent'[2] ELSE FALSE)
Next == ((\E i\in Ids: Select(i) \/ Decision(i) \/ Ack(i) \/ Commit(i) \/ Confirm(i)
          \/ DropL(i) \/ DropF(i) \/ DataL(i) \/ DataF(i) \/ BadEpoch(i)) \/ CloseL \/ CloseF) /\ Project
Spec == Init /\ [][Next]_vars
HealthyNext == (\E i\in Ids: Select(i) \/ Decision(i) \/ Ack(i) \/ Commit(i) \/ Confirm(i)) /\ Project
HealthySpec == Init /\ [][HealthyNext]_vars /\ (\A i\in Ids:
                 /\ WF_vars(Select(i) /\ Project) /\ WF_vars(Decision(i) /\ Project)
                 /\ WF_vars(Ack(i) /\ Project) /\ WF_vars(Commit(i) /\ Project)
                 /\ WF_vars(Confirm(i) /\ Project))
BothReady == <>(pl /\ pf)
TypeOK == /\ l\in [Ids->0..5] /\ f\in [Ids->1..5] /\ d\in [Ids->0..2] /\ a\in [Ids->0..2]
          /\ c\in [Ids->0..1] /\ sent\in [Ids->BOOLEAN] /\ wl\in 0..Count /\ wf\in 0..Count
          /\ firstl\in 0..Count /\ firstf\in 0..Count /\ pl\in BOOLEAN /\ pf\in BOOLEAN
          /\ cl\in BOOLEAN /\ cf\in BOOLEAN /\ dl\in 0..Count /\ df\in 0..Count
          /\ badEpoch\in BOOLEAN /\ badClosed\in BOOLEAN /\ event\in 0..12 /\ item\in 0..Count
SingleSelection == /\ wl=firstl /\ wf=firstf
                   /\ \A i\in Ids:(l[i]=3 => wl=i) /\ (f[i]=3 => wf=i)
Agreement == (pl /\ pf) => wl=wf
AgreedPublication == /\ pl => wl=wf /\ wf#0
                     /\ pf => wl=wf /\ wl#0
ConfirmedFollower == pf => sent[wf]
DataCommitted == /\ dl#0 => pl /\ l[dl]=3
                 /\ df#0 => pf /\ f[df]=3
EpochBound == ~badEpoch
NoReactivation == ~badClosed
=============================================================================
