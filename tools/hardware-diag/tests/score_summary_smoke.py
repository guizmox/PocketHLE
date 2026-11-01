"""Run the actual ARM menu and score parser with controlled completed reports.

No timings or emulator API correctness are inferred from these UI-only shims.
"""
from pathlib import Path
import struct
# Reuse the API shims without running the other suites on import.
namespace={"__file__":str(Path(__file__).with_name('guest_smoke.py'))}
exec(Path(__file__).with_name('guest_smoke.py').read_text().split('ROOT=')[0],namespace)
Guest=namespace['Guest']
ROOT=Path(__file__).resolve().parents[1]
EXE=ROOT/'build/card/GZHD999989/AUTORUN.EXE'
class PageGuest(Guest):
 def __init__(self,suite,report,filename,keys=None):
  super().__init__(EXE);self.suite=suite;self.files['\\Flash Disk\\'+filename]=bytearray(report.encode());self.frames=[];self.frame=[];self.keys=keys
 def hook(self,u,a,size,data):
  name=self.apis.get(a)
  if name=='PatBlt':self.frame=[]
  if name=='TextOutW':self.frame.append((self.arg(1),self.arg(2),self.wide(self.arg(3))[:self.arg(4)]))
  if name=='ReleaseDC':self.frames.append(self.frame.copy())
  if name=='CreateThread' and self.suite in (9,10,12):assert self.frames[-1]==[(100,114,'Running benchmark...')],self.frames[-1]
  if name=='Sleep':
   self.sleeps+=1
   if self.sleeps==1:self.messages=[40]*self.suite+[13]
   if self.sleeps==5 and self.keys:self.messages=self.keys
   if self.sleeps==8:u.emu_stop()
   u.reg_write(namespace['UC_ARM_REG_R0'],1);u.reg_write(namespace['UC_ARM_REG_PC'],u.reg_read(namespace['UC_ARM_REG_LR']));return
  super().hook(u,a,size,data)
def report(suite,complete=True,fail=False):
 if suite==12:
  return ''.join(f'MEM_CASE_ID={i}\nMEM_MEDIAN_MILLI={1000000+i}\nMEM_CASE_RESULT '+('FAIL' if fail and i==5 else 'PASS')+'\n' for i in range(10))+('MEMORY_RESULT PASS\n' if complete else '')
 if suite<11:
  name='INTEGER' if suite==9 else 'FLOATING';n=8 if suite==9 else 10
  s=f'FINAL_{name}_SCORE_ITERATIONS_PER_SECOND=312153208\n'
  for i in range(n):s+=f'CASE_ID={i}\nCASE_MEDIAN_ITERATIONS_PER_SECOND={1000000+i}\nCASE_RESULT '+('FAIL' if fail and i==5 else 'PASS')+'\n'
  return s+('BENCHMARK_RESULT PASS\n' if complete else '')
 ids=['geometry_512','geometry_2048','geometry_8192','draw_calls_128','matrix_animated','vertices_rebuilt','fill_solid','fill_texture_nearest','fill_texture_linear','fill_texture_blend']
 return ''.join(f'TEST={v}\nMEDIAN_RENDER_MILLIFPS={1000000+i}\nTEST_RESULT PASS\n' for i,v in enumerate(ids))+'GLOBAL_INDEX_MILLI=2881215\n'+('OGLESBENCH_RESULT PASS\n' if complete else '')
for suite,filename,title in [(9,'INTEGER.TXT','INTEGER'),(10,'FLOATING.TXT','FLOATING POINT'),(11,'OGLESBENCH.TXT','OPENGLES'),(12,'MEMORY.TXT','MEMORY')]:
 g=PageGuest(suite,report(suite),filename);g.run();rows=g.frames[-1];texts=[t for x,y,t in rows]
 assert title in texts and 'PASS' in texts,texts
 assert ('312 153 208' if suite<11 else '2881.215' if suite==11 else '1000.000 MiB/s') in texts,texts
 assert ('1 000 005' if suite<11 else '1000.009' if suite==11 else '1000.009 ns') in texts,texts
 for x,y,t in rows:assert 0<=x and x+len(t)*6<=320 and y+11<=240,(x,y,t)
 for complete,fail in [(False,False),(True,True) if suite!=11 else (False,False)]:
  g=PageGuest(suite,report(suite,complete,fail),filename);g.run();assert 'FAIL' in [t for x,y,t in g.frames[-1]],g.frames[-1]
 g=PageGuest(suite,report(suite),filename,[9]);g.run();assert any('B/Tab: scores' in t for x,y,t in g.frames[-1]),g.frames[-1]
 g=PageGuest(suite,report(suite),filename,[9,9]);g.run();assert title in [t for x,y,t in g.frames[-1]],g.frames[-1]
 print(title+': ARM summary scores, screen bounds, failed/truncated report, log toggle PASS')

class AllGuest(Guest):
 def hook(self,u,a,size,data):
  if self.apis.get(a)=='Sleep':
   self.sleeps+=1
   if self.sleeps==1:self.messages=[40]*13+[13]
   if self.sleeps==45:u.emu_stop()
   u.reg_write(namespace['UC_ARM_REG_R0'],1);u.reg_write(namespace['UC_ARM_REG_PC'],u.reg_read(namespace['UC_ARM_REG_LR']));return
  super().hook(u,a,size,data)
g=AllGuest(EXE);g.run();assert [c[2] for c in g.created]==list(range(13)),g.created
print('Run all: all thirteen suites selected on same-process workers, including memory PASS')
