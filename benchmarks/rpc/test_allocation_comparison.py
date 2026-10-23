"""Acceptance checks for comparisons of saved profiles, without running benchmarks."""
import copy
import unittest
from allocation_comparison import compare


def fixture():
    summary = dict(run={'headSha': 'synthetic'}, profile_peers=[], profiles=[])
    scope = dict(trials=[], allocation_workloads=[])
    for transport in ['tls', 'quic']:
        scope['allocation_workloads'].append(dict(transport=transport, case='chain', points=[5, 50]))
        for variant, blocks in [('legacy', 9), ('portable', 10), ('cpp', 8)]:
            if transport == 'quic' and variant == 'cpp':
                continue
            for iterations in [5, 50]:
                scope['trials'].append(dict(mode='dhat', transport=transport, case='chain',
                    implementation=variant, iterations=iterations, points=[5, 50],
                    services=[dict(hop=i) for i in range(4)]))
            for peer in ['client', 'service-0', 'service-1', 'service-2', 'service-3']:
                summary['profile_peers'].append(dict(mode='dhat', transport=transport, case='chain',
                    variant=variant, peer=peer, points=[5, 50], per_operation=dict(blocks=blocks, bytes=16*blocks)))
            summary['profiles'].append(dict(mode='dhat', transport=transport, case='chain',
                variant=variant, per_operation=dict(blocks=5*blocks, bytes=80*blocks)))
    return summary, scope


class AllocationComparisonTests(unittest.TestCase):
    def test_lower_comparator_and_all_peer_batch_counts(self):
        result = compare(*fixture())
        tls, quic = result['rows']
        self.assertEqual((tls['status'], tls['target_variant'], tls['excess_blocks']), ('gap', 'cpp', 5))
        self.assertEqual((quic['status'], quic['target_variant'], quic['excess_blocks']), ('met', 'portable', -5))
        self.assertEqual((result['met'], result['gaps'], result['unmeasured']), (1, 1, 0))
        self.assertFalse(result['allocation_target_met'])

    def test_missing_required_comparator_or_service_cannot_pass(self):
        for missing in ['comparator', 'peer', 'short_trial']:
            summary, scope = fixture()
            if missing == 'comparator':
                summary['profiles'] = [r for r in summary['profiles'] if r['variant'] != 'cpp']
            elif missing == 'peer':
                summary['profile_peers'] = [r for r in summary['profile_peers']
                    if not (r['transport'] == 'tls' and r['variant'] == 'legacy' and r['peer'] == 'service-3')]
            else:
                scope['trials'] = [r for r in scope['trials']
                    if not (r['transport'] == 'tls' and r['implementation'] == 'legacy' and r['iterations'] == 5)]
            result = compare(summary, scope)
            self.assertEqual(result['rows'][0]['status'], 'unmeasured', missing)
            self.assertFalse(result['allocation_target_met'])

    def test_inconsistent_total_is_rejected_and_equality_passes(self):
        summary, scope = fixture()
        broken = copy.deepcopy(summary)
        broken['profiles'][0]['per_operation']['blocks'] -= 1
        with self.assertRaisesRegex(ValueError, 'all-peer total'):
            compare(broken, scope)
        for row in summary['profiles'] + summary['profile_peers']:
            if row['variant'] == 'legacy':
                row['per_operation'] = {k: v*8/9 for k,v in row['per_operation'].items()}
        result = compare(summary, scope)
        self.assertTrue(result['allocation_target_met'])
        self.assertEqual(result['rows'][0]['excess_blocks'], 0)


if __name__ == '__main__':
    unittest.main()
