#!/usr/bin/env python3
"""Generate finite TLC configurations for the three feature-contract models."""
from pathlib import Path
ROOT = Path(__file__).resolve().parent.parent

def write(name, module, constants, invariants, properties=(), violation=None):
    text = f'\\* @module: {module}\n'
    if violation:
        text += f'\\* @violation: {violation}\n'
    text += 'SPECIFICATION ' + ('LiveSpec' if properties else 'Spec') + '\nCONSTANTS\n'
    for key, value in constants.items():
        literal = ('TRUE' if value else 'FALSE') if isinstance(value,bool) else (str(value) if isinstance(value,int) else '"'+value+'"')
        text += f' {key} = {literal}\n'
    text += 'CHECK_DEADLOCK FALSE\nINVARIANTS\n ' + '\n '.join([violation] if violation else invariants) + '\n'
    if properties:
        text += 'PROPERTIES\n ' + '\n '.join(properties) + '\n'
    (ROOT/'verification/configs'/(name+'.cfg')).write_text(text)

schema_inv = ['TypeOK','SchemaIntegrity','DependencyClosure','PinnedRevision','NoRejectedUse']
bulk_inv = ['TypeOK','WindowBound','CreditConservation','InOrderExactlyOnce','StickyErrors','CompletionSound','TerminalCleanup']
crypto_inv = ['TypeOK','AuthenticatedDelivery','CapabilityBinding','FreshSession','AtMostOnce','NonceUnique','Confidentiality']

def schema(name, prop='SuccessfulFetch', violation=None, **changes):
    constants = dict(Revision=1,Scenario='normal',AllowCorruption=False,Bug='none')
    constants.update(changes)
    write(name,'CapnpSchemaExchange',constants,schema_inv,[prop] if prop and not violation else [],violation)

def bulk(name, violation=None, **changes):
    constants = dict(ChunkCount=4,Window=3,FailAt=0,AllowCancel=False,AllowDuplicateAck=False,Bug='none')
    constants.update(changes)
    write(name,'CapnpBulkTransfer',constants,bulk_inv,[] if violation else ['Settles'],violation)

def crypto(name, violation=None, **changes):
    constants = dict(EpochCount=1,MessageCount=1,Attack='none',Bug='none')
    constants.update(changes)
    write(name,'CapnpEncryptedTransport',constants,crypto_inv,[] if violation else ['Completes'],violation)

def main():
    schema('SchemaExchange')
    schema('SchemaRevisionTwo',Revision=2)
    schema('SchemaCycle',Scenario='cycle')
    schema('SchemaShared',Scenario='shared')
    schema('SchemaMissing','MissingFails',Scenario='missing')
    schema('SchemaCorruption','Settles',AllowCorruption=True)
    schema('SchemaBugRevision',violation='SchemaIntegrity',AllowCorruption=True,Bug='wrongRevision')
    schema('SchemaBugEarly',violation='DependencyClosure',Bug='earlyActivation')
    schema('SchemaWitness',violation='NoSchemaWitness')
    schema('SchemaWitnessCycle',violation='NoCycleWitness',Scenario='cycle')
    schema('SchemaWitnessReject',violation='NoRejectionWitness',AllowCorruption=True)
    schema('SchemaWitnessPinnedUpgrade',violation='NoPinnedUpgradeWitness')
    bulk('BulkTransfer')
    bulk('BulkSmallWindow',Window=2)
    bulk('BulkWideWindow',ChunkCount=6,Window=5)
    bulk('BulkCancellation',AllowCancel=True)
    bulk('BulkError',FailAt=2)
    bulk('BulkDuplicateAck',AllowDuplicateAck=True)
    bulk('BulkCancelError',AllowCancel=True,FailAt=2,AllowDuplicateAck=True)
    bulk('BulkBugWindow',violation='WindowBound',Bug='ignoreWindow')
    bulk('BulkBugCredit',violation='CreditConservation',Bug='doubleCredit',AllowDuplicateAck=True)
    bulk('BulkBugError',violation='StickyErrors',Bug='forgetError',FailAt=2)
    bulk('BulkWitness',violation='NoBulkWitness')
    bulk('BulkWitnessCancel',violation='NoCancelWitness',AllowCancel=True)
    bulk('BulkWitnessBackpressure',violation='NoBackpressureWitness')
    bulk('BulkWitnessError',violation='NoErrorWitness',FailAt=2)
    crypto('EncryptedTransport',MessageCount=2)
    for attack in ['forge','tamper','replay','capability','epoch']:
        crypto('Encrypted'+attack.title(),Attack=attack,EpochCount=2 if attack=='epoch' else 1)
    crypto('EncryptedRotation',EpochCount=2)
    crypto('EncryptedConcurrentRotation',EpochCount=2,MessageCount=2,Attack='replay')
    crypto('EncryptedBugAuthentication',violation='AuthenticatedDelivery',Attack='forge',Bug='skipAuthentication')
    crypto('EncryptedBugTamper',violation='AuthenticatedDelivery',Attack='tamper',Bug='skipAuthentication')
    crypto('EncryptedBugCapability',violation='CapabilityBinding',Attack='capability',Bug='unboundCapability')
    crypto('EncryptedBugReplay',violation='AtMostOnce',Attack='replay',Bug='replay')
    crypto('EncryptedBugEpoch',violation='FreshSession',Attack='epoch',EpochCount=2,Bug='staleSession')
    crypto('EncryptedBugNonce',violation='NonceUnique',MessageCount=2,Bug='reuseNonce')
    crypto('EncryptedBugCleartext',violation='Confidentiality',Bug='cleartext')
    crypto('EncryptedWitnessZeroRtt',violation='NoZeroRttWitness')
    crypto('EncryptedWitnessReplay',violation='NoReplayRejectionWitness',Attack='replay')
    crypto('EncryptedWitnessRotation',violation='NoRotationWitness',EpochCount=2)

if __name__ == '__main__':
    main()
