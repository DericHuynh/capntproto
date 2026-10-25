"""Reconcile only exact default client/future and hosted reply alias spellings."""
import json
from pathlib import Path
import re
import statistics
import sys

PREFIX = re.compile(r'capnp::capability::(?:Promise(?:Inner)?|Client)<|capnp_rpc::incoming::Params<|capntproto_rpc_engine::completion::direct::receiver::Receiver<')

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
    while match := PREFIX.search(text, pos):
        args, end = arguments(text, match.end())
        args = [canonical(a) for a in args]
        name = match.group()[:-1]
        if name in ('capnp::capability::Promise', 'capnp::capability::PromiseInner') and len(args) == 3:
            expected = ('core::pin::Pin<alloc::boxed::Box<dyn core::future::future::Future<'
                        f'Output = core::result::Result<{args[0]}, {args[1]}>>>>')
            if re.sub(r'\s+', '', args[2]) == re.sub(r'\s+', '', expected):
                args = args[:2]
        elif name == 'capnp::capability::Client' and args == ['alloc::boxed::Box<dyn capnp::private::capability::ClientHook>']:
            args = []
        elif name == 'capnp_rpc::incoming::Params' and len(args) == 2:
            boxed = 'alloc::boxed::Box<'
            caps = 'alloc::vec::Vec<core::option::Option<alloc::boxed::Box<dyn capnp::private::capability::ClientHook>>>'
            if args[0].startswith(boxed) and args[0].endswith('>') and args[1] == caps:
                name, args = 'capnp_rpc::rpc::Params', [args[0][len(boxed):-1]]
        elif name.endswith('::receiver::Receiver') and len(args) == 2:
            prefix = 'alloc::rc::Rc<capnp_rpc::rpc::QuestionCell<'
            if args[0].startswith(prefix) and args[0].endswith('>>'):
                vat = args[0][len(prefix):-2]
                expected = f'capnp::capability::Promise<capnp_rpc::rpc::Response<{vat}>, capnp::Error>'
                if args[1] == expected:
                    name, args = 'capnp_rpc::rpc::QuestionReply', [vat]
        result += text[pos:match.start()] + name + ('<' + ', '.join(args) + '>' if args else '')
        pos = end
    return result + text[pos:]


def summary(samples):
    return dict(samples=samples, mean=statistics.mean(samples), sd=statistics.stdev(samples))


def assembly(path):
    result = {}
    name = None
    for line in path.read_text().splitlines():
        header = re.match(r'^([0-9a-f]+) <(.*)>:$', line)
        if header:
            address, name = header.groups()
            result[name] = dict(address=int(address, 16), instructions=[], indirect_calls=[])
        elif name:
            match = re.match(r'^\s*([0-9a-f]+):\s+(.*)$', line)
            if match:
                address, instruction = match.groups()
                address = int(address, 16)
                instruction = instruction.split('#', 1)[0].strip()
                instruction = re.sub(r'-?0x[0-9a-f]+\(%rip\)', '<RIP-relative>', instruction)
                instruction = re.sub(r'\b[0-9a-f]+ (<.*>)', r'\1', instruction)
                result[name]['instructions'].append([address - result[name]['address'], instruction])
                if re.match(r'call\s+\*', instruction):
                    result[name]['indirect_calls'].append(dict(address=address, predictor_index=address & 511))
    return result


def main(root):
    full = (root/'functions.json').exists()
    source = 'functions.json' if full else 'functions-selected.json'
    rows, aliases = [], {}
    for row in json.loads((root/source).read_text()):
        original_functions = row['functions' if full else 'selected_paths']
        grouped = {}
        for function in original_functions:
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
        before = sum(f['paired_delta']['mean'] for f in original_functions)
        after = sum(f['paired_delta']['mean'] for f in functions)
        assert abs(before-after) < 1e-6, (before,after)
        functions.sort(key=lambda f:abs(f['paired_delta']['mean']), reverse=True)
        rows.append(dict(case=row['case'], metric=row['metric'], source_function_count=len(original_functions),
                         canonical_function_count=len(functions), attributed_delta=after, top_changes=functions[:20]))
    method = ('Only exact default Box<dyn ClientHook>, default boxed Promise futures and the exact hosted '
              'QuestionReply = Receiver<Rc<QuestionCell<Vat>>,Promise<Response<Vat>,Error>> and the hosted '
              'rpc::Params<M> = incoming::Params<Box<M>,Vec<Option<Box<dyn ClientHook>>>> aliases are equated. Other concrete/borrowed owners remain distinct. Sum each of five samples before statistics; '
              'conserve source attribution. Selected input covers selected paths only; it is not whole-program attribution.')
    (root/'owner-name-analysis.json').write_text(json.dumps(dict(method=method, source=source, aliases=aliases, rows=rows), indent=2)+'\n')
    print('Reconciled',len(aliases),'exact owner spellings across',len(rows),'case/metric rows from',source)
    if (root/'factored.assembly').exists() and (root/'candidate.assembly').exists():
        snapshots = [assembly(root/(label+'.assembly')) for label in ['factored','candidate']]
        maps = [{canonical(name):(name, body) for name,body in snapshot.items()} for snapshot in snapshots]
        assert all(len(mapped)==len(original) for mapped,original in zip(maps,snapshots)), 'ambiguous canonical symbol'
        pairs = []
        for name in sorted(maps[0].keys() & maps[1].keys()):
            (old,a),(new,b) = maps[0][name],maps[1][name]
            forms = [[[offset,canonical(instruction)] for offset,instruction in body['instructions']] for body in [a,b]]
            pairs.append(dict(function=name, baseline_name=old, candidate_name=new,
                              renamed=old!=new, normalized_structure_equal=forms[0]==forms[1],
                              instruction_count=[len(f) for f in forms]))
        result = dict(method='Instruction offsets/forms with linked addresses/RIP displacements normalized and only exact owner spellings reconciled. Not byte equivalence or hardware latency.',
                      pairs=pairs, baseline_only=sorted(maps[0].keys()-maps[1].keys()), candidate_only=sorted(maps[1].keys()-maps[0].keys()))
        (root/'owner-assembly-analysis.json').write_text(json.dumps(result,indent=2)+'\n')
        print('Canonical assembly common/same',len(pairs),sum(p['normalized_structure_equal'] for p in pairs))


if __name__ == '__main__':
    main(Path(sys.argv[1]))
