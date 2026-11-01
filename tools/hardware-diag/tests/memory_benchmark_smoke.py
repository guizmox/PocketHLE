"""Actual ARM memory workloads with deterministic clocks/API adapters.

This verifies execution, checksums, rates/units and failure handling. It is not
an emulator/hardware performance result. Run after build.py.
"""
from pathlib import Path
import struct,re
namespace={'__file__':str(Path(__file__).with_name('guest_smoke.py'))}
exec(Path(__file__).with_name('guest_smoke.py').read_text().split('ROOT=')[0],namespace)
Guest=namespace['Guest'];R=namespace['R'];SP=namespace['UC_ARM_REG_SP'];PC=namespace['UC_ARM_REG_PC'];LR=namespace['UC_ARM_REG_LR']
ROOT=Path(__file__).resolve().parents[1]
class MemoryGuest(Guest):
 def __init__(self,corrupt=False,allocation_failure=0,backwards=False):
  super().__init__(ROOT/'build/card/GZHD999989/AUTORUN.EXE');self.command='--hd-memory';self.allocations={};self.alloc_calls=0;self.corrupt=corrupt;self.allocation_failure=allocation_failure;self.backwards=backwards;self.ticks=0
 def hook(self,u,a,size,data):
  name=self.apis.get(a);v=1;handled=True
  if name=='VirtualAlloc':
   self.alloc_calls+=1;assert self.arg(0)==0 and self.arg(2)==0x3000 and self.arg(3)==4
   if self.alloc_calls==self.allocation_failure:v=0
   else:
    n=(self.arg(1)+4095)&~4095;v=0x20000000+self.alloc_calls*0x1000000;u.mem_map(v,n);self.allocations[v]=n
  elif name=='VirtualFree':
   p=self.arg(0);assert p in self.allocations and self.arg(1)==0 and self.arg(2)==0x8000
   u.mem_unmap(p,self.allocations.pop(p))
  elif name=='memcpy':
   p,src,n=self.arg(0),self.arg(1),self.arg(2);u.mem_write(p,bytes(u.mem_read(src,n)));v=p
   if self.corrupt:u.mem_write(p,b'\x00')
  elif name=='memset':p,byte,n=self.arg(0),self.arg(1)&255,self.arg(2);u.mem_write(p,bytes([byte])*n);v=p
  elif name=='GetTickCount':
   self.ticks+=1
   # Three distinct durations/rates: exercise median calculation, not a mean.
   self.time+=100 if self.ticks%3 else 200
   v=(0x80000000 if self.ticks%2==0 else 1000) if self.backwards else self.time
  else:handled=False
  if handled:u.reg_write(R[0],v);u.reg_write(PC,u.reg_read(LR))
  else:super().hook(u,a,size,data)
 def report(self):return bytes(self.files['\\Flash Disk\\MEMORY.TXT']).decode()
g=MemoryGuest();g.run();s=g.report();assert g.exit==0 and 'MEMORY_RESULT PASS' in s,s
assert not g.allocations and not g.created and len(re.findall(r'^MEM_CASE_ID=',s,re.M))==10
sections=re.split(r'^MEM_CASE_ID=',s,flags=re.M)[1:]
for id,section in enumerate(sections):
 rates=list(map(int,re.findall(r'^mem_rate_milli=(\d+)',section,re.M)));median=int(re.search(r'^MEM_MEDIAN_MILLI=(\d+)',section,re.M)[1]);assert len(rates)==3 and median==sorted(rates)[1]
 rows=list(zip(map(int,re.findall(r'^mem_elapsed_ms=(\d+)',section,re.M)),map(int,re.findall(r'^mem_blocks=(\d+)',section,re.M))))
 for (ms,blocks),actual in zip(rows,rates):
  expected=((16384 if id==0 else 262144 if id==1 else 2097152)*blocks*1000000//(ms*1048576) if id<7 else 16384*blocks//ms if id==7 else ms*1000000000//(16384*blocks))
  assert actual==expected,(id,actual,expected)
print('Memory ARM: all 10 fixed workloads, units, rates, 3-pass medians, checksums/guards and cleanup PASS')
g=MemoryGuest(corrupt=True);g.run();s=g.report();assert g.exit!=0 and 'MEMORY_RESULT FAIL' in s
copy=re.split(r'^MEM_CASE_ID=5\r?\n',s,flags=re.M)[1].split('MEM_CASE_ID=6')[0]
assert 'MEM_CASE_RESULT FAIL' in copy and 'MEM_MEDIAN_MILLI' not in copy and not g.allocations
print('Memory ARM: corrupt HLE copy fails and suppresses the affected score PASS')
for call in [1,2]:
 g=MemoryGuest(allocation_failure=call);g.run();s=g.report();assert g.exit!=0 and 'FAIL allocation' in s and 'MEM_MEDIAN' not in s and not g.allocations
print('Memory ARM: partial/complete allocation failures clean up without fabricated scores PASS')
g=MemoryGuest(backwards=True);g.run();s=g.report();assert g.exit!=0 and 'MEM_MEDIAN_MILLI' not in s and not g.allocations
print('Memory ARM: invalid/backwards clocks suppress scores PASS')
