"""Checked self-event attribution and repeated marginal-work summaries.

Callgrind counters are simulated; none of these values qualify native latency.
Keep negative slopes: independent process startup does not cancel exactly.
"""
from collections import defaultdict
import math
import re
import statistics


def callgrind(text):
    names, functions = {}, defaultdict(lambda: defaultdict(int))
    current = None
    skip_call = False
    events = positions = summary = totals = None
    for line in text.splitlines():
        if line.startswith('events:'):
            events = line.split()[1:]
        elif line.startswith('positions:'):
            positions = len(line.split()) - 1
        elif line.startswith('summary:'):
            summary = list(map(int, line.split()[1:]))
        elif line.startswith('totals:'):
            totals = list(map(int, line.split()[1:]))
        elif line.startswith(('fn=', 'cfn=')):
            match = re.fullmatch(r'(c?fn)=\((\d+)\)(?: (.*))?', line)
            assert match, line
            kind, index, name = match.groups()
            if name is not None:
                names[index] = name
            if kind == 'fn':
                current = index
        elif line.startswith('calls='):
            skip_call = True
        elif line and line[0] in '0123456789+-*':
            if skip_call:
                skip_call = False
                continue  # Arc costs include callees; do not count them twice.
            assert events is not None and positions is not None and current is not None
            values = list(map(int, line.split()[positions:]))
            assert len(values) <= len(events)
            # Callgrind omits trailing zero counters, especially branch misses.
            values += [0] * (len(events) - len(values))
            for event, value in zip(events, values):
                functions[current][event] += value
    assert events is not None and summary is not None and totals is not None
    assert len(events) == len(summary) == len(totals)
    result = defaultdict(lambda: defaultdict(int))
    for event, total in zip(events, totals):
        assert sum(row[event] for row in functions.values()) == total, event
    for index, counts in functions.items():
        name = names[index]
        if re.fullmatch(r'0x[0-9a-fA-F]+', name):
            name = '[unresolved instruction address]'
        for event, count in counts.items():
            result[name][event] += count
    for event, total, count in zip(events, totals, summary):
        result['[summary minus attributed total]'][event] += count - total
    return dict(zip(events, summary)), dict(result)


def spread(values):
    assert values and all(math.isfinite(value) for value in values)
    return dict(mean=statistics.mean(values), sd=statistics.stdev(values) if len(values) > 1 else 0,
                minimum=min(values), maximum=max(values), samples=values)


def slope(short, long, points):
    assert 0 < points[0] < points[1]
    return {name: (long.get(name, 0) - short.get(name, 0)) / (points[1] - points[0])
            for name in sorted(short.keys() | long.keys())}


def comparisons(rows, pairs):
    grouped = defaultdict(dict)
    for row in rows:
        key = row['mode'], row['transport'], row['case']
        identity = row['variant'], row['repetition']
        assert identity not in grouped[key], (key, identity)
        grouped[key][identity] = row['counts']
    result = []
    for key, captures in grouped.items():
        repetitions = sorted({r for _, r in captures})
        for before, after in pairs:
            left = [captures[before, r] for r in repetitions]
            right = [captures[after, r] for r in repetitions]
            assert all(row.keys() == left[0].keys() for row in [*left, *right])
            metrics = {}
            for metric in left[0]:
                baseline = [r[metric] for r in left]
                candidate = [r[metric] for r in right]
                delta = [b - a for a, b in zip(baseline, candidate)]
                mean_before = statistics.mean(baseline)
                metrics[metric] = dict(baseline=spread(baseline), candidate=spread(candidate),
                                       paired_delta=spread(delta),
                                       change_percent=100 * statistics.mean(delta) / mean_before if mean_before else None)
            result.append(dict(mode=key[0], transport=key[1], case=key[2], baseline=before,
                               candidate=after, repetitions=len(repetitions), metrics=metrics))
    return result
