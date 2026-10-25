"""Compare selected disassembly structure and simulated-predictor indices."""
import hashlib
import json
from pathlib import Path
import re
import sys

root=Path(sys.argv[1])
def functions(path):
    result={}
    name=None
    for line in path.read_text().splitlines():
        header=re.match(r'^([0-9a-f]+) <(.*)>:$',line)
        if header:
            start,name=header.groups()
            result[name]={'address':int(start,16),'instructions':[],'indirect_sites':[]}
        elif name:
            instruction=re.match(r'^\s*([0-9a-f]+):\s+(.*)$',line)
            if not instruction:continue
            address,text=instruction.groups()
            address=int(address,16)
            offset=address-result[name]['address']
            # Compare structure, not linked bytes. RIP displacements and linked
            # call addresses can change independently of the instruction forms.
            text=text.split('#',1)[0].strip()
            text=re.sub(r'-?0x[0-9a-f]+\(%rip\)', '<RIP-relative>',text)
            text=re.sub(r'\b[0-9a-f]+ (<.*>)',r'\1',text)
            result[name]['instructions'].append([offset,text])
            if re.match(r'(?:call|jmp)\s+\*',text):
                result[name]['indirect_sites'].append(dict(offset=offset,address=address,index=address&511,instruction=text))
    return result

frozen=functions(root/'frozen.assembly')
factored=functions(root/'factored.assembly')
candidate=functions(root/'candidate.assembly')
rows=[]
for name in sorted(frozen.keys()&factored.keys()):
    a,b=frozen[name],factored[name]
    if not any(f in name for f in ['ReturnGuard','answer_has_sent_return','aes_gcm::aeshwclmulmovbe::seal','::poll_prepare_frame']):
        continue
    rows.append(dict(function=name,frozen_address=a['address'],factored_address=b['address'],
                     instruction_count=[len(a['instructions']),len(b['instructions'])],
                     normalized_structure_equal=a['instructions']==b['instructions'],
                     frozen_indirect_sites=a['indirect_sites'],factored_indirect_sites=b['indirect_sites']))
(root/'layout-comparison.json').write_text(json.dumps(dict(method='Instruction offsets and forms; linked addresses/RIP displacements normalized. Not byte equivalence. Predictor indices use address & 511, matching the referenced Valgrind x86-64 indirect simulator.',rows=rows),indent=2)+'\n')
for row in rows:
    print(row['function'],row['normalized_structure_equal'],row['instruction_count'],[(x['index'],y['index']) for x,y in zip(row['frozen_indirect_sites'],row['factored_indirect_sites'])])
