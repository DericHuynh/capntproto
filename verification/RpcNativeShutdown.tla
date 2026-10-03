------------------------ MODULE RpcNativeShutdown ------------------------
EXTENDS Naturals, TLC
CONSTANTS Bytes, Crossed, Fault
VARIABLES drained, sent, seen, data, peerData, peerSeen, ack, ackSeen,
          reply, replySeen, confirmed, done, failed, bad, late, event
vars == <<drained,sent,seen,data,peerData,peerSeen,ack,ackSeen,reply,replySeen,
          confirmed,done,failed,bad,late,event>>
Init == /\ drained=FALSE /\ sent=FALSE /\ seen=FALSE /\ data=0 /\ peerData=0
        /\ peerSeen=FALSE /\ ack=FALSE /\ ackSeen=FALSE /\ reply=FALSE
        /\ replySeen=FALSE /\ confirmed=FALSE /\ done=FALSE /\ failed=FALSE
        /\ bad=FALSE /\ late=FALSE /\ event=0
Open == ~done /\ ~failed
Drain == /\ Open /\ ~drained /\ drained'=TRUE /\ event'=1
         /\ UNCHANGED <<sent,seen,data,peerData,peerSeen,ack,ackSeen,reply,replySeen,confirmed,done,failed,bad,late>>
Send == /\ Open /\ ~sent /\ (drained \/ Fault="undrained") /\ sent'=TRUE /\ event'=2
        /\ UNCHANGED <<drained,seen,data,peerData,peerSeen,ack,ackSeen,reply,replySeen,confirmed,done,failed,bad,late>>
Request == /\ Open /\ sent /\ ~seen /\ seen'=TRUE /\ event'=3
           /\ UNCHANGED <<drained,sent,data,peerData,peerSeen,ack,ackSeen,reply,replySeen,confirmed,done,failed,bad,late>>
Data == /\ Open /\ data<Bytes /\ data'=data+1 /\ event'=4
        /\ UNCHANGED <<drained,sent,seen,peerData,peerSeen,ack,ackSeen,reply,replySeen,confirmed,done,failed,bad,late>>
Ack == /\ Open /\ seen /\ ~ack /\ (data=Bytes \/ Fault="earlyAck") /\ ack'=TRUE /\ event'=5
       /\ UNCHANGED <<drained,sent,seen,data,peerData,peerSeen,ackSeen,reply,replySeen,confirmed,done,failed,bad,late>>
Receive == /\ Open /\ ack /\ ~ackSeen /\ ackSeen'=TRUE /\ event'=6
           /\ UNCHANGED <<drained,sent,seen,data,peerData,peerSeen,ack,reply,replySeen,confirmed,done,failed,bad,late>>
PeerRequest == /\ Open /\ Crossed /\ ~peerSeen /\ peerSeen'=TRUE /\ event'=7
               /\ UNCHANGED <<drained,sent,seen,data,peerData,ack,ackSeen,reply,replySeen,confirmed,done,failed,bad,late>>
PeerData == /\ Open /\ Crossed /\ peerData<Bytes /\ peerData'=peerData+1 /\ event'=8
            /\ UNCHANGED <<drained,sent,seen,data,peerSeen,ack,ackSeen,reply,replySeen,confirmed,done,failed,bad,late>>
Reply == /\ Open /\ sent /\ peerSeen /\ ~reply /\ (peerData=Bytes \/ Fault="earlyReply")
         /\ reply'=TRUE /\ event'=9
         /\ UNCHANGED <<drained,sent,seen,data,peerData,peerSeen,ack,ackSeen,replySeen,confirmed,done,failed,bad,late>>
ReplyReceive == /\ Open /\ reply /\ ~replySeen /\ replySeen'=TRUE /\ event'=10
                /\ UNCHANGED <<drained,sent,seen,data,peerData,peerSeen,ack,ackSeen,reply,confirmed,done,failed,bad,late>>
Confirm == /\ Open /\ replySeen /\ ~confirmed /\ confirmed'=TRUE /\ event'=11
           /\ UNCHANGED <<drained,sent,seen,data,peerData,peerSeen,ack,ackSeen,reply,replySeen,done,failed,bad,late>>
Finish == /\ Open /\ ackSeen /\ (~Crossed \/ confirmed \/ Fault="crossed")
          /\ done'=TRUE /\ event'=12
          /\ UNCHANGED <<drained,sent,seen,data,peerData,peerSeen,ack,ackSeen,reply,replySeen,confirmed,failed,bad,late>>
PeerClose == /\ Open /\ Crossed /\ ackSeen /\ replySeen /\ done'=TRUE /\ event'=13
             /\ UNCHANGED <<drained,sent,seen,data,peerData,peerSeen,ack,ackSeen,reply,replySeen,confirmed,failed,bad,late>>
Fail == /\ Open /\ failed'=TRUE /\ event'=14
        /\ UNCHANGED <<drained,sent,seen,data,peerData,peerSeen,ack,ackSeen,reply,replySeen,confirmed,done,bad,late>>
BadAck == /\ Open /\ ~bad /\ bad'=TRUE /\ failed'=(Fault#"binding")
          /\ ackSeen'=(ackSeen \/ Fault="binding") /\ event'=15
          /\ UNCHANGED <<drained,sent,seen,data,peerData,peerSeen,ack,reply,replySeen,confirmed,done,late>>
Late == /\ failed /\ ~late /\ late'=TRUE /\ done'=(Fault="resurrect") /\ event'=16
        /\ UNCHANGED <<drained,sent,seen,data,peerData,peerSeen,ack,ackSeen,reply,replySeen,confirmed,failed,bad>>
HealthyNext == Drain \/ Send \/ Request \/ Data \/ Ack \/ Receive \/ PeerRequest
               \/ PeerData \/ Reply \/ ReplyReceive \/ Confirm \/ Finish \/ PeerClose
Next == HealthyNext \/ Fail \/ BadAck \/ Late
Spec == Init /\ [][Next]_vars
LiveSpec == Init /\ [][HealthyNext]_vars /\ WF_vars(HealthyNext)
TypeOK == /\ data\in 0..Bytes /\ peerData\in 0..Bytes /\ event\in 0..16
          /\ \A x\in {drained,sent,seen,peerSeen,ack,ackSeen,reply,replySeen,confirmed,done,failed,bad,late}: x\in BOOLEAN
DrainFence == sent => drained
ReceiptFence == ack => seen /\ data=Bytes
ReplyFence == reply => peerSeen /\ peerData=Bytes
Binding == ackSeen => ack /\ sent
CrossedFence == done /\ Crossed => replySeen
Terminal == done => ~failed
Success == done => ackSeen /\ sent /\ drained /\ data=Bytes
Progress == <>done
=============================================================================
