"""Actual ARM bytecode smoke checks with controlled API/clock shims.

Build HARDWARE DIAG first, then install tests/requirements.txt and run this file.
These checks verify the integrated menu/benchmark/role wiring, not PocketHLE's
WinCE implementations, real timings, renderer or hardware providers.
"""
from pathlib import Path
import struct,collections,sys
import pefile
from unicorn import Uc,UC_ARCH_ARM,UC_MODE_ARM,UC_HOOK_CODE
from unicorn.arm_const import *
R=[UC_ARM_REG_R0,UC_ARM_REG_R1,UC_ARM_REG_R2,UC_ARM_REG_R3]
class Guest:
 def __init__(self,path):
  self.u=Uc(UC_ARCH_ARM,UC_MODE_ARM);self.pe=pefile.PE(str(path));self.files={};self.handles={};self.next=100;self.command="";self.time=0;self.error=0;self.calls=collections.Counter();self.text=[];self.messages=[];self.proc=0;self.sleeps=0;self.created=[]
  for s in self.pe.sections:
   a=self.pe.OPTIONAL_HEADER.ImageBase+s.VirtualAddress;n=(max(s.Misc_VirtualSize,s.SizeOfRawData)+4095)&~4095;self.u.mem_map(a,n);self.u.mem_write(a,s.get_data())
  self.u.mem_map(0x70000000,0x10000);self.u.mem_map(0x800000,0x10000);self.u.reg_write(UC_ARM_REG_SP,0x80fff0);self.u.reg_write(UC_ARM_REG_LR,0x7000fff0);self.apis={}
  for d in self.pe.DIRECTORY_ENTRY_IMPORT:
   for i in d.imports:
    a=0x70000000+len(self.apis)*32;self.apis[a]=i.name.decode();self.u.mem_write(i.address,struct.pack('<I',a));self.u.mem_write(a,b'\x1e\xff\x2f\xe1')
  self.u.hook_add(UC_HOOK_CODE,self.hook,begin=0x70000000,end=0x7000ffff)
 def arg(self,n):return self.u.reg_read(R[n]) if n<4 else self.word(self.u.reg_read(UC_ARM_REG_SP)+4*(n-4))
 def word(self,p):return struct.unpack('<I',self.u.mem_read(p,4))[0]
 def put(self,p,v):self.u.mem_write(p,struct.pack('<I',v&0xffffffff))
 def wide(self,p):
  b=bytearray()
  while p and self.u.mem_read(p,2)!=b'\0\0':b.extend(self.u.mem_read(p,2));p+=2
  return b.decode('utf-16le')
 def hook(self,u,a,size,data):
  if a==0x7000fff0:self.exit=self.arg(0);u.emu_stop();return
  name=self.apis[a];self.calls[name]+=1;v=1
  if name=='GetCommandLineW':
   u.mem_write(0x7000f000,self.command.encode('utf-16le')+b'\0\0');v=0x7000f000
  elif name=='GetTickCount':self.time+=250;v=self.time
  elif name=='GetLastError':v=self.error
  elif name=='SetLastError':self.error=self.arg(0)
  elif name=='CreateFileW':
   p=self.wide(self.arg(0));disp=self.arg(4)
   if disp==3 and p not in self.files:v=0xffffffff
   else:
    if disp==2 or p not in self.files:self.files[p]=bytearray()
    v=self.next;self.next+=1;self.handles[v]=[p,0]
  elif name=='CloseHandle':self.handles.pop(self.arg(0),None)
  elif name=='SetFilePointer':
   h=self.handles[self.arg(0)];off=self.arg(1);origin=self.arg(3);h[1]=(len(self.files[h[0]]) if origin==2 else h[1] if origin==1 else 0)+off;v=h[1]
  elif name in ('ReadFile','WriteFile'):
   h=self.handles[self.arg(0)];p=self.arg(1);n=self.arg(2);b=self.files[h[0]]
   if name=='ReadFile':chunk=bytes(b[h[1]:h[1]+n]);n=len(chunk);u.mem_write(p,chunk) if n else None
   else:
    chunk=bytes(u.mem_read(p,n));end=h[1]+n
    if end>len(b):b.extend(b'\0'*(end-len(b)))
    b[h[1]:end]=chunk
   h[1]+=n;self.put(self.arg(3),n)
  elif name=='__rt_udiv':v=self.arg(1)//self.arg(0) if self.arg(0) else 0
  elif name in ('__adds','__muls'):
   f=lambda x:struct.unpack('<f',struct.pack('<I',x))[0];x,y=f(self.arg(0)),f(self.arg(1));v=struct.unpack('<I',struct.pack('<f',x+y if name=='__adds' else x*y))[0]
  elif name=='RegisterClassW':self.proc=self.word(self.arg(0)+4)
  elif name=='CreateWindowExW':v=0xdead0001
  elif name=='TextOutW':self.text.append(self.wide(self.arg(3))[:self.arg(4)])
  elif name=='PeekMessageW':
   if self.messages:
    vk=self.messages.pop(0);u.mem_write(self.arg(0),struct.pack('<7I',0xdead0001,0x100,vk,0,0,0,0));v=1
   else:v=0
  elif name=='DispatchMessageW':
   p=self.arg(0);args=struct.unpack('<4I',u.mem_read(p,16))
   for r,x in zip(R,args):u.reg_write(r,x)
   u.reg_write(UC_ARM_REG_PC,self.proc);return
  elif name=='CreateThread':
   self.created.append(('thread',self.arg(2),self.arg(3)));self.put(self.arg(5),502);self.child=0;v=500
  elif name=='CreateProcessW':
   self.created.append(self.wide(self.arg(0)));u.mem_write(self.arg(9),struct.pack('<4I',500,501,502,503));self.child=0
  elif name=='WaitForSingleObject':self.child+=1;v=0 if self.child>1 else 258
  elif name in ('GetExitCodeProcess','GetExitCodeThread'):self.put(self.arg(1),0)
  elif name=='Sleep':
   self.sleeps+=1
   if getattr(self,'ui',False):
    if self.sleeps==1:self.messages=[40]*9+[13]
    if self.sleeps==8:u.emu_stop()
  elif name in ('TranslateMessage','DefWindowProcW'):v=0
  elif name not in ('PatBlt','GetDC','ReleaseDC','SelectObject','SetBkMode','SetTextColor','ValidateRect','CreateFontIndirectW','GetStockObject','ShowWindow','SetFocus','TerminateProcess','DestroyWindow','PostQuitMessage'):raise AssertionError('Unexpected API '+name)
  u.reg_write(UC_ARM_REG_R0,v);u.reg_write(UC_ARM_REG_PC,u.reg_read(UC_ARM_REG_LR))
 def run(self):
  self.u.mem_write(0x7000d000,self.command.encode('utf-16le')+b'\0\0')
  for r,v in zip(R,[0x10000,0,0x7000d000,1]):self.u.reg_write(r,v)
  self.u.emu_start(self.pe.OPTIONAL_HEADER.ImageBase+self.pe.OPTIONAL_HEADER.AddressOfEntryPoint,0,timeout=20000000)
ROOT=Path(__file__).resolve().parents[1]
root=ROOT/'build/card/GZHD999989'
for mode in ['integer','floating']:
 g=Guest(root/'AUTORUN.EXE');g.command='--hd-'+mode;g.run();s=bytes(g.files['\\Flash Disk\\'+('INTEGER.TXT' if mode=='integer' else 'FLOATING.TXT')]).decode();assert 'BENCHMARK_RESULT PASS' in s and g.exit==0,s;assert s.count('elapsed_ms=')==3;assert not g.created;print(mode, 'integrated ARM benchmark: 3 passes + warmup PASS; no child process')
g=Guest(root/'AUTORUN.EXE');g.ui=True;g.run();assert 'HARDWARE DIAG' in g.text;assert len(g.created)==1 and g.created[0][0]=='thread',g.created;assert not g.calls['CreateProcessW'];assert 'SUITE FINISHED (exit 0)' in g.text;print('integrated ARM UI: menu, controller selection, same-process worker, completion, console PASS')
# Execute the real same-image process adapter, preserving its ten-argument ABI.
import importlib.util
spec=importlib.util.spec_from_file_location('pe_builder',ROOT/'suites/ram/build.py');builder=importlib.util.module_from_spec(spec);spec.loader.exec_module(builder)
symbols=builder.symbols(ROOT/'build/integrated/image.elf')
class SpawnGuest(Guest):
 def hook(self,u,a,size,data):
  name=self.apis.get(a)
  if name=='GetModuleFileNameW':
   s='\\SD Card\\GZHD999989\\AUTORUN.EXE';u.mem_write(self.arg(1),s.encode('utf-16le')+b'\0\0');u.reg_write(UC_ARM_REG_R0,len(s));u.reg_write(UC_ARM_REG_PC,u.reg_read(UC_ARM_REG_LR));return
  if name=='CreateProcessW':
   self.created.append((self.wide(self.arg(0)),self.wide(self.arg(1)),self.arg(5)));u.reg_write(UC_ARM_REG_R0,1);u.reg_write(UC_ARM_REG_PC,u.reg_read(UC_ARM_REG_LR));return
  super().hook(u,a,size,data)
for suffix,mode,flag in [('ram\\proctest.exe','suite','proctest'),('ram\\procworker.exe','d','procworker'),('vfs\\vfsworker.exe','e','vfsworker')]:
 g=SpawnGuest(root/'AUTORUN.EXE');g.u.mem_write(0x7000e000,('\\SD Card\\GZHD999989\\'+suffix).encode('utf-16le')+b'\0\0');g.u.mem_write(0x7000e200,mode.encode('utf-16le')+b'\0\0')
 for reg,val in zip(R,[0x7000e000,0x7000e200,0,0]):g.u.reg_write(reg,val)
 sp=g.u.reg_read(UC_ARM_REG_SP)-64;g.u.reg_write(UC_ARM_REG_SP,sp);g.u.mem_write(sp,struct.pack('<6I',0,4,0,0,0,0x7000e400));g.u.emu_start(symbols['hardware_diag_spawn'],0,timeout=1000000)
 assert g.created==[('\\SD Card\\GZHD999989\\AUTORUN.EXE','--hd-'+flag+' '+mode,4)],g.created
print('integrated ARM process fixtures: SAME EXE for all 3 child roles; flags preserved PASS')
