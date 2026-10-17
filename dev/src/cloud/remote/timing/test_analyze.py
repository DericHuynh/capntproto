import struct
import unittest
from analyze import EVENT, intervals


def journal(events):
    return struct.pack('<Q', len(events)) + b''.join(EVENT.pack(*e) for e in events)


def event(start, end, cs, ce, result=8, fd=4, op=1, tid=7, reserved=0):
    return start, end, cs, ce, result, fd, op, tid, reserved


class Pairing(unittest.TestCase):
    def test_pairs_only_successful_io_on_same_thread_and_descriptor(self):
        data = journal([
            event(10, 20, 1, 2),
            event(21, 22, 3, 4, op=4, tid=8),
            event(23, 24, 5, 6, op=4, fd=5),
            event(25, 26, 7, 8, result=-1),
            event(30, 40, 10, 11, op=4),
            event(41, 42, 12, 13, op=4),
            event(50, 60, 14, 15, op=7),
            event(65, 70, 16, 17, op=9),
            event(80, 90, 20, 21, op=10),
        ])
        result = intervals(data)
        self.assertEqual(result['wall_ns'], [10, 10])
        self.assertEqual(result['cpu_ns'], [8, 3])
        self.assertEqual(result['thread_ids'], [7])
        self.assertEqual(result['event_count'], 9)

    def test_rejects_truncated_overflowed_or_invalid_journals(self):
        valid = journal([event(10, 20, 1, 2)])
        for data in [b'', valid[:-1], valid + b'0', struct.pack('<Q', 131073),
                     journal([event(20, 10, 1, 2)]),
                     journal([event(10, 20, 3, 2)]),
                     journal([event(10, 20, 1, 2, tid=0)]),
                     journal([event(10, 20, 1, 2, op=13)]),
                     journal([event(10, 20, 1, 2, reserved=1)]),
                     journal([event(10, 20, 5, 6), event(21, 22, 3, 4, op=4)])]:
            with self.subTest(data=data), self.assertRaises(ValueError):
                intervals(data)


if __name__ == '__main__':
    unittest.main()
