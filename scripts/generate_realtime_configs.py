#!/usr/bin/env python3
"""Generate TLC checks for the proposed realtime snapshot contract."""
from generate_feature_configs import write

INVARIANTS = ['TypeOK','PacketIntegrity','NoLateApplication','AtMostOnce','NoRollback','QueueBound',
              'OnePendingPerKey','PendingFreshness','ReceiptSoundness',
              'ReceiptsConsistent','NeverApplyCanceled','CloseBarrier','AppliedOutcome']

def case(name, violation=None, **changes):
    constants = dict(MessageCount=2,KeyCount=1,Capacity=1,SubmitBefore=1,Lifetime=2,
                     ClockSkew=0,AllowLoss=False,AllowDuplicate=False,
                     AllowCancel=False,AllowClose=False,Bug='none')
    constants.update(changes)
    write(name,'CapnpRealtime',constants,INVARIANTS,
          [] if violation else ['AllOffersDecide','CloseCompletes','EventuallyDrained'],violation)

def main():
    case('Realtime')
    case('RealtimeClockSkew',ClockSkew=1)
    case('RealtimeMultipleKeys',MessageCount=3,KeyCount=2,Capacity=2)
    case('RealtimeCapacity',KeyCount=2)
    case('RealtimeLoss',AllowLoss=True)
    case('RealtimeDuplicates',AllowDuplicate=True)
    case('RealtimeCancellation',AllowCancel=True)
    case('RealtimeClose',AllowClose=True)
    case('RealtimeDelayedSubmission',SubmitBefore=2)
    case('RealtimeCombined',AllowLoss=True,AllowDuplicate=True,AllowCancel=True,AllowClose=True)
    case('RealtimeBugDeadline','NoLateApplication',Bug='admissionOnly')
    case('RealtimeBugClock','NoLateApplication',Bug='unsafeClock',ClockSkew=1)
    case('RealtimeBugStale','NoRollback',Bug='staleApply')
    case('RealtimeBugDuplicate','AtMostOnce',Bug='duplicateApply',AllowDuplicate=True)
    case('RealtimeBugCapacity','QueueBound',Bug='overflow',KeyCount=2)
    case('RealtimeBugCancel','NeverApplyCanceled',Bug='forgetCancel',AllowCancel=True)
    case('RealtimeBugClose','CloseBarrier',Bug='closeLeak',AllowClose=True)
    case('RealtimeBugTimeout','ReceiptSoundness',Bug='timeoutClaimsDrop')
    case('RealtimeWitnessApply','NoAppliedWitness')
    case('RealtimeWitnessExpire','NoExpiredWitness')
    case('RealtimeWitnessSupersede','NoSupersededWitness')
    case('RealtimeWitnessBusy','NoBusyWitness',KeyCount=2)
    case('RealtimeWitnessCancel','NoCanceledWitness',AllowCancel=True)
    case('RealtimeWitnessUnknown','NoUnknownWitness')
    case('RealtimeWitnessLateReceipt','NoLateReceiptWitness')
    case('RealtimeWitnessClose','NoClosedWitness',AllowClose=True)
    case('RealtimeWitnessCancelAfterApply','NoCancelAfterApplyWitness',AllowCancel=True)
    case('RealtimeWitnessLoss','NoLostWitness',AllowLoss=True)

if __name__ == '__main__':
    main()
