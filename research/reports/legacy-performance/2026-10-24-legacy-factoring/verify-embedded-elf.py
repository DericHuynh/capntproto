from pathlib import Path
import struct, json, hashlib, argparse
parser=argparse.ArgumentParser();parser.add_argument('--elf',type=Path,default=Path('/tmp/legacy-factor-embedded.elf'));parser.add_argument('--source',type=Path,default=Path(__file__).with_name('embedded-link.rs'));args=parser.parse_args()
p=args.elf;data=p.read_bytes()
assert data[:6]==b'\x7fELF\x01\x01'
offset=struct.unpack_from('<I',data,32)[0]
size,count=struct.unpack_from('<HH',data,46)
sections=[struct.unpack_from('<IIIIIIIIII',data,offset+i*size) for i in range(count)]
symbols=[];undefined=[]
for section in sections:
 if section[1]!=2:continue
 strings=sections[section[6]];names=data[strings[4]:strings[4]+strings[5]]
 for at in range(section[4],section[4]+section[5],section[9]):
  name,value,length,info,other,index=struct.unpack_from('<IIIBBH',data,at)
  text=names[name:].split(b'\0',1)[0].decode()
  if text:
   symbols.append(text)
   if index==0:undefined.append(text)
allocators=[s for s in symbols if any(k in s for k in ['__rust_alloc','__rust_dealloc','__rust_realloc','__rg_alloc','__rdl_alloc','__rust_alloc_error_handler'])]
assert not undefined,undefined
assert not allocators,allocators
assert '_start' in symbols
result={'target':'thumbv7em-none-eabi','artifact':str(p),'bytes':len(data),'sha256':hashlib.sha256(data).hexdigest(),'undefined_symbols':undefined,'allocator_symbols':allocators,'entry_present':True,'source_sha256':hashlib.sha256(args.source.read_bytes()).hexdigest(),'scope':'Link-only, not executed on a board. Borrowed payload and core-only direct completion; black-boxed owner references prevent eliminating the entire completion sequence.'}
print(json.dumps(result,indent=2))
