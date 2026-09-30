#!/usr/bin/env python3
"""Regenerate finite workload configurations for the composed RPC model."""
from pathlib import Path
ROOT = Path(__file__).resolve().parent.parent
invariants=['TypeOK','ProtocolSafety','ReferenceConservation','NoStaleCredits','QuestionLifetime','AnswerLifetime','AtMostOnce','CancellationSafety','EmbargoSafety','AcceptAuthority','JoinAgreement','DisconnectClean','EOrder','JoinResources','ExpectedSuccess','ExportedQuestionLifetime','VinesHeldUntilAcceptance']
def cfg(name,plan,vats=2,ids=3,ops=4,third=False,bug='none',expected=None,live=False):
 text='\\* @module: CapnpNetworkChecks\n'
 if expected:text+='\\* @violation: '+expected+'\n'
 expect_success = plan not in {'BrokenPromisePlan','PersistencePlan','LossPlan','UnsupportedPlan','UnauthorizedAcceptPlan','InvalidPipelinePlan','ProvideLossPlan','AdoptedLossPlan','AdoptionFailurePlan','ProviderLossHandoffPlan'} and bug != 'rejectCall'
 text+='SPECIFICATION '+('LiveSpec' if live else 'Spec')+'\nCONSTANTS\n'
 text+=f' VatCount = {vats}\n IdCount = {ids}\n MaxOps = {ops}\n Plan <- {plan}\n Bug = "{bug}"\n AllowLoss = FALSE\n Introductions = '+str(third).upper()+'\n ExpectSuccess = '+str(expect_success).upper()+'\nCHECK_DEADLOCK FALSE\nINVARIANTS\n'
 text+='\n'.join(' '+n for n in ([expected] if expected else invariants))+'\n'
 if live:text+='PROPERTIES\n PlanCompletes\n EveryQuestionSettles\n EveryInterestedResultSettles\n AdoptedCleanup\n AutomaticHandoffsComplete\n'
 if live and third:text+=' HandoffResourcesReleased\n ProvidedResourcesReleased\n'
 (ROOT / 'verification' / 'configs' / (name+'.cfg')).write_text(text)
cfg('NetworkBasic','BasicPlan',live=True)
cfg('NetworkCancel','CancelPlan',live=True)
cfg('NetworkPipeline','PipelinePlan',ops=4,live=True)
cfg('NetworkIds','SameNumericIdsPlan',vats=3,ops=3,ids=1,live=True)
cfg('NetworkLocalTail','TailLocalPlan',ops=4,live=True)
cfg('NetworkThirdTail','ThirdTailPlan',vats=3,ops=6,live=True)
cfg('NetworkHandoff','HandoffPlan',vats=3,ops=9,ids=4,third=True,live=True)
cfg('NetworkJoin','JoinPlan',vats=4,ops=12,ids=4,live=True)

for name,plan,vats,ids,ops in [
 ('NoFinish','NoFinishPlan',2,2,3),('Only','OnlyPlan',2,2,3),('OnlyCompat','OnlyCompatPlan',2,2,3),
 ('NoPipeline','NoPipelinePlan',2,2,2),('ExplicitRelease','ExplicitReleasePlan',2,3,2),
 ('Workaround','WorkaroundPlan',2,3,3),('BrokenPromise','BrokenPromisePlan',2,3,3),
 ('Persistence','PersistencePlan',2,3,5),('Loss','LossPlan',2,1,1),('Unsupported','UnsupportedPlan',2,1,1),
 ('UnauthorizedAccept','UnauthorizedAcceptPlan',2,1,1),('Provide','DirectProvidePlan',3,2,4),
 ('TailFallback','ThirdTailFallbackPlan',3,3,5),('TailCancel','ThirdTailCancelPlan',3,3,6),
 ('TailPipeline','ThirdTailPipelinePlan',3,3,7),
 ('LocalJoin','LocalJoinPlan',2,4,4),('UnequalJoin','UnequalJoinPlan',2,4,4),('CancelJoin','CanceledJoinPlan',2,4,4)]:
 cfg('Network'+name,plan,vats=vats,ids=ids,ops=ops,live=True)
cfg('NetworkBugNamespace','ThirdTailPlan',vats=3,ops=6,bug='adoptionNamespace',expected='ProtocolSafety')
cfg('NetworkBugJoin','UnequalJoinPlan',ops=4,ids=4,bug='hostOnlyJoin',expected='JoinAgreement')
cfg('NetworkWitnessUnequal','UnequalJoinPlan',ops=4,ids=4,expected='NoUnequalNetworkWitness')
cfg('NetworkWitnessRestore','PersistencePlan',ops=5,expected='NoRestoreNetworkWitness')
cfg('NetworkWitnessAdoption','ThirdTailPlan',vats=3,ops=6,expected='NoEarlyAdoptionWitness')
cfg('NetworkWitnessTailCleanup','ThirdTailPlan',vats=3,ops=6,expected='NoCleanupNetworkWitness')

cfg('NetworkNestedPipeline','NestedPipelinePlan',ops=3,live=True)
cfg('NetworkInvalidPipeline','InvalidPipelinePlan',ops=3,live=True)

cfg('NetworkLoopback','LoopbackPlan',ops=5,live=True)
cfg('NetworkResolve','ReleasedResolvePlan',ops=2,live=True)
cfg('NetworkBugEmbargo','HandoffPlan',vats=3,ops=9,ids=4,third=True,bug='skipEmbargo',expected='EmbargoSafety')
cfg('NetworkBugBarrierOrder','HandoffPlan',vats=3,ops=9,ids=4,third=True,bug='barrierOvertakes',expected='EOrder')
cfg('NetworkWitnessHandoff','HandoffPlan',vats=3,ops=9,ids=4,third=True,expected='NoHandoffWitness')
cfg('NetworkWitnessJoin','JoinPlan',vats=4,ops=12,ids=4,expected='NoJoinWitness')
cfg('NetworkWitnessLoopback','LoopbackPlan',ops=5,expected='NoLoopbackNetworkWitness')
cfg('NetworkWitnessReleasedResolve','ReleasedResolvePlan',ops=2,expected='NoReleasedResolveNetworkWitness')
cfg('NetworkWitnessAcceptRace','DirectProvidePlan',vats=3,ops=4,ids=2,expected='NoAcceptBeforeProvideNetworkWitness')

for name in ['Pair','Duplicate','ReceiverAnswer']:
 cfg('Network'+name,name+'Plan',ops=3,live=True)

cfg('NetworkExportedQuestion','ExportedQuestionPlan',vats=3,ops=4,ids=3,live=True)

cfg('NetworkAdoptedPipeline','AdoptedPipelinePlan',vats=3,ops=6,ids=3,live=True)

cfg('NetworkUnimplementedCall','BasicPlan',ops=2,bug='rejectCall',live=True)
cfg('NetworkUnimplementedResolve','ReleasedResolvePlan',ops=2,bug='rejectResolve',live=True)

cfg('NetworkProvideLoss','ProvideLossPlan',vats=3,ops=3,ids=2,live=True)

cfg('NetworkBugExportedQuestion','ExportedQuestionPlan',vats=3,ops=4,bug='finishExportedQuestion',expected='ExportedQuestionLifetime')

cfg('NetworkUnresolvedTailCancel','UnresolvedTailCancelPlan',vats=3,ops=6,live=True)
cfg('NetworkWitnessEarlyAdoptedFinish','UnresolvedTailCancelPlan',vats=3,ops=6,expected='NoEarlyAdoptedFinishWitness')

cfg('NetworkAdoptedLoss','AdoptedLossPlan',vats=3,ops=6,live=True)
cfg('NetworkAdoptionFailure','AdoptionFailurePlan',vats=3,ops=6,live=True)
cfg('NetworkWitnessAdoptionFailure','AdoptionFailurePlan',vats=3,ops=6,expected='NoAdoptionFailureWitness')

cfg("NetworkReturnHandoff","ReturnHandoffPlan",vats=3,ids=3,ops=7,third=True,live=True)
cfg("NetworkParameterHandoff","ParameterHandoffPlan",vats=3,ids=2,ops=6,third=True,live=True)
cfg("NetworkWitnessRuntimeHandoff","HandoffPlan",vats=3,ids=4,ops=9,third=True,expected="NoRuntimeHandoffCallWitness")
cfg("NetworkCanceledReturnHandoff","CanceledReturnHandoffPlan",vats=3,ids=3,ops=5,third=True,live=True)
cfg("NetworkProviderLossHandoff","ProviderLossHandoffPlan",vats=3,ids=3,ops=7,third=True,live=True)
