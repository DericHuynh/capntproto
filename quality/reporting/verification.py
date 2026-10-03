"""Graphs from actual fuzz engine counters and validated TLC check logs."""
import hashlib
from pathlib import Path
import re


def chart(name, title, unit, note, panels):
    return dict(name=name, title=title, unit=unit, note=note,
                panels=[dict(title=title, bars=[dict(label=k, value=v) for k, v in values])
                        for title, values in panels])


def models(data, directory):
    records = data.get('models', [])
    if not records:
        return []
    counts = dict(positive=0, controls=0, failed=0)
    states = {}
    for record in records:
        if not re.fullmatch(r'[a-f0-9]{64}', record['id']):
            raise ValueError('invalid model identity')
        counts['controls' if record['expected_exit'] else 'positive'] += bool(record['passed'])
        counts['failed'] += not record['passed']
        if record['log_sha256'] is None:
            if record['passed']:
                raise ValueError('passing TLC result without a log')
            continue
        log = (Path(directory) / 'models' / f"{record['id']}.log").read_bytes()
        if hashlib.sha256(log).hexdigest() != record['log_sha256']:
            raise ValueError('TLC log hash mismatch')
        # Record the final TLC summary, not intermediate progress or queue size.
        matches = re.findall(r'([\d,]+) states generated, ([\d,]+) distinct states found', log.decode())
        if matches:
            generated, distinct = (int(value.replace(',', '')) for value in matches[-1])
            module = Path(record['module']).stem
            totals = states.setdefault(module, [0, 0])
            totals[0] += generated
            totals[1] += distinct
    result = [chart('tla-outcomes', 'TLA+ checks and expected counterexamples', 'Checks',
                    'Unique module/configuration/expected-exit checks. Expected invariant violations are successful controls.',
                    [('Observed outcomes', [('Invariant/liveness checks', counts['positive']),
                                            ('Expected counterexamples', counts['controls']),
                                            ('Failed / incomplete checks', counts['failed'])])])]
    if states:
        top = sorted(states, key=lambda module: states[module][1], reverse=True)[:15]
        panels = []
        for index, title in enumerate(('Generated states', 'Distinct states')):
            values = [(module, states[module][index]) for module in top]
            if len(states) > len(top):
                values.append(('Other models', sum(v[index] for k, v in states.items() if k not in top)))
            panels.append((title, values))
        result.append(chart('tla-states', 'TLA+ explored states by model', 'States',
                            'Counts sum independent bounded configurations; they are not globally distinct states or a proof beyond those bounds.', panels))
    return result


def fuzz(data):
    runs, coverage, findings = [], [], []
    libfuzzer = data.get('libfuzzer', {}).get('campaigns', {})
    for target, campaign in sorted(libfuzzer.items()):
        label = f'libFuzzer / {target}'
        runs.append((label, campaign['runs']))
        match = re.search(r'\bcov:\s*(\d+)', campaign['coverage_feedback'])
        if match:
            coverage.append((label, int(match[1])))
        # This manifest is emitted only after the crash-artifact inventory is empty.
        findings.append((label, 0))
    afl_runs, afl_coverage, afl_findings = [], [], []
    for campaign in data.get('afl', {}).get('campaigns', []):
        stats = campaign['statistics']
        if stats is None:
            continue
        label = f'AFL++ / {campaign["target"]}'
        afl_runs.append((label, stats['execs_done']))
        afl_coverage.append((label, stats['edges_found']))
        afl_findings.extend([(label + ' crashes', stats['saved_crashes']),
                             (label + ' hangs', stats['saved_hangs'])])
    result = []
    for name, title, unit, note, left, right in [
        ('fuzz-executions', 'Fuzzing executions by engine', 'Executions',
         'Bounded campaigns including seed calibration; execution counts are not comparable performance benchmarks.', runs, afl_runs),
        ('fuzz-coverage', 'Fuzzer feedback by engine', 'Feedback entries',
         'Engine-local counters, not LLVM source coverage. Do not compare counts across engines or builds.', coverage, afl_coverage),
        ('fuzz-findings', 'Saved fuzzing findings', 'Inputs',
         'Saved crashes/hangs are findings requiring triage, not confirmed unique bugs. Missing results remain unknown.', findings, afl_findings),
    ]:
        panels = [(engine, values) for engine, values in [('libFuzzer + ASan', left), ('AFL++ + RPC IJON', right)] if values]
        if panels:
            result.append(chart(name, title, unit, note, panels))
    return result
