------------------------- MODULE NoiseCloseFlush -------------------------
EXTENDS Naturals
CONSTANT Fault
VARIABLES receipt, closeSent, done, turns, event
vars == <<receipt,closeSent,done,turns,event>>
Init == /\ receipt=0 /\ closeSent=0 /\ done=0 /\ turns=0 /\ event=0
Receipt == /\ receipt=0 /\ receipt'=1 /\ event'=1
           /\ UNCHANGED <<closeSent,done,turns>>
SendClose == /\ receipt=1 /\ closeSent=0 /\ done=0
             /\ closeSent'=1 /\ event'=2 /\ UNCHANGED <<receipt,done,turns>>
Turn == /\ receipt=1 /\ done=0 /\ turns<2 /\ turns'=turns+1 /\ event'=3
        /\ done'=(IF closeSent=1 /\ Fault="forgetReceipt" THEN 2 ELSE 0)
        /\ UNCHANGED <<receipt,closeSent>>
Flush == /\ receipt=1 /\ done=0 /\ (closeSent=1 \/ Fault="earlyCompletion")
         /\ done'=1 /\ event'=4 /\ UNCHANGED <<receipt,closeSent,turns>>
Next == Receipt \/ SendClose \/ Turn \/ Flush
Spec == Init /\ [][Next]_vars
TypeOK == /\ receipt\in 0..1 /\ closeSent\in 0..1 /\ done\in 0..2
          /\ turns\in 0..2 /\ event\in 0..4
ReceiptRetained == done#2
CompletionSound == done=1 => receipt=1 /\ closeSent=1
=============================================================================
