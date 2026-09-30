------------------------- MODULE NoiseScheduling -------------------------
EXTENDS Naturals
CONSTANT Fault
VARIABLES credits, capacity, time, configured, same, closed, attempts, admitted,
          grant, packets, turns, packetTry, event
vars == <<credits,capacity,time,configured,same,closed,attempts,admitted,
          grant,packets,turns,packetTry,event>>
Init == /\ credits=2 /\ capacity=2 /\ time=0 /\ configured=0 /\ same=0
        /\ closed=0 /\ attempts=0 /\ admitted=0 /\ grant=0
        /\ packets=0 /\ turns=0 /\ packetTry=0 /\ event=0
Take == /\ attempts<3 /\ attempts'=attempts+1 /\ event'=1
        /\ LET allowed == credits>0 /\ (closed=0 \/ Fault="afterClose")
           IN /\ grant'=IF allowed THEN 1 ELSE 0
              /\ credits'=IF allowed THEN credits-1 ELSE credits
              /\ admitted'=IF allowed THEN admitted+1 ELSE admitted
        /\ UNCHANGED <<capacity,time,configured,same,closed,packets,turns,packetTry>>
Tick == /\ time<1 /\ closed=0 /\ time'=time+1 /\ event'=2
        /\ credits'=IF credits<capacity THEN credits+1 ELSE credits
        /\ UNCHANGED <<capacity,configured,same,closed,attempts,admitted,grant,packets,turns,packetTry>>
Configure == /\ configured=0 /\ closed=0 /\ configured'=1 /\ capacity'=1 /\ event'=3
             /\ credits'=IF credits>1 THEN 1 ELSE credits
             /\ UNCHANGED <<time,same,closed,attempts,admitted,grant,packets,turns,packetTry>>
Same == /\ same=0 /\ closed=0 /\ same'=1 /\ event'=4
        /\ credits'=IF Fault="freeCredit" THEN capacity ELSE credits
        /\ UNCHANGED <<capacity,time,configured,closed,attempts,admitted,grant,packets,turns,packetTry>>
Packet == /\ packetTry<3 /\ closed=0 /\ packetTry'=packetTry+1 /\ event'=5
          /\ packets'=IF packets<2 \/ Fault="burstOverflow" THEN packets+1 ELSE packets
          /\ UNCHANGED <<credits,capacity,time,configured,same,closed,attempts,admitted,grant,turns>>
Yield == /\ turns=0 /\ closed=0 /\ turns'=1 /\ packets'=0 /\ event'=6
         /\ UNCHANGED <<credits,capacity,time,configured,same,closed,attempts,admitted,grant,packetTry>>
Close == /\ closed=0 /\ closed'=1 /\ event'=7
         /\ UNCHANGED <<credits,capacity,time,configured,same,attempts,admitted,grant,packets,turns,packetTry>>
Next == Take \/ Tick \/ Configure \/ Same \/ Packet \/ Yield \/ Close
Spec == Init /\ [][Next]_vars
TypeOK == /\ credits\in 0..2 /\ capacity\in 1..2 /\ time\in 0..1
          /\ configured\in 0..1 /\ same\in 0..1 /\ closed\in 0..1
          /\ attempts\in 0..3 /\ admitted\in 0..3 /\ grant\in 0..1
          /\ packets\in 0..3 /\ turns\in 0..1 /\ packetTry\in 0..3 /\ event\in 0..7
CreditBound == credits<=capacity /\ admitted<=2+time
BurstBound == packets<=2
NoAfterClose == closed=1 /\ event=1 => grant=0
=============================================================================
