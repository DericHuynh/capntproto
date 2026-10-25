"""Group only equivalent explicit default Promise future type spellings."""
import json
from pathlib import Path
import re
import statistics
import sys

root = Path(sys.argv[1])
prefix = re.compile(r'capnp::capability::Promise(?:Inner)?<')


def arguments(text, start):
    depth, begin, parts = 1, start, []
    groups = []
    for i in range(start, len(text)):
        char = text[i]
        if char in '([{':
            groups.append(char)
        elif char in ')]}':
            if groups:
                groups.pop()
        elif not groups:
            if char == '<':
                depth += 1
            elif char == '>' and text[i-1] != '-':
                depth -= 1
                if not depth:
                    return [*parts, text[begin:i].strip()], i + 1
            elif char == ',' and depth == 1:
                parts.append(text[begin:i].strip())
                begin = i + 1
    raise ValueError(text[start:])


def canonical(text):
    result, pos = '', 0
    while match := prefix.search(text, pos):
        args, end = arguments(text, match.end())
        args = [canonical(a) for a in args]
        if len(args) == 3:
            expected = ('core::pin::Pin<alloc::boxed::Box<dyn core::future::future::Future<'
                        f'Output = core::result::Result<{args[0]}, {args[1]}>>>>')
            if re.sub(r'\s+', '', args[2]) == re.sub(r'\s+', '', expected):
                args = args[:2]
        result += text[pos:match.end()] + ', '.join(args) + '>'
        pos = end
    return result + text[pos:]


def summary(samples):
    return dict(samples=samples, mean=statistics.mean(samples), sd=statistics.stdev(samples))


rows, aliases = [], {}
for row in json.loads((root/'functions.json').read_text()):
    grouped = {}
    for function in row['functions']:
        original = function['function']
        name = canonical(original)
        if name != original:
            aliases[original] = name
        target = grouped.setdefault(name, dict(function=name, baseline=[0.0]*5, candidate=[0.0]*5))
        for label in ['baseline', 'candidate']:
            target[label] = [a+b for a,b in zip(target[label], function[label]['samples'])]
    functions = []
    for target in grouped.values():
        a, b = target['baseline'], target['candidate']
        functions.append(dict(function=target['function'], baseline=summary(a), candidate=summary(b),
                              paired_delta=summary([y-x for x,y in zip(a,b)])))
    before = sum(f['paired_delta']['mean'] for f in row['functions'])
    after = sum(f['paired_delta']['mean'] for f in functions)
    assert abs(before-after) < 1e-6, (before,after)
    functions.sort(key=lambda f:abs(f['paired_delta']['mean']), reverse=True)
    rows.append({**{k:v for k,v in row.items() if k != 'functions'},
                 'raw_function_count':len(row['functions']), 'canonical_function_count':len(functions),
                 'attributed_delta':after, 'top_changes':functions[:20]})
report = dict(method='Collapse a third Promise/PromiseInner argument only when it exactly equals its default Pin<Box<dyn Future<Output=Result<T,E>>>> after recursively normalizing default spellings. Every sample is summed before computing mean/SD. Whole deltas are conserved; other futures are not equated.',
              aliases=aliases, rows=rows)
(root/'function-name-analysis.json').write_text(json.dumps(report,indent=2)+'\n')
for row in rows:
    if row['case']=='deep-32-concurrent-32' and row['metric'] in ['Ir','Bc']:
        print(row['metric'],row['attributed_delta'],[(f['paired_delta']['mean'],f['function']) for f in row['top_changes'][:6]])
