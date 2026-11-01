"""Actual ARM bytecode smoke checks with controlled API/clock shims.

Build HARDWARE DIAG first, then install tests/requirements.txt and run this file.
These checks verify the integrated menu/benchmark/role wiring, not PocketHLE's
WinCE implementations, real timings, renderer or hardware providers.
"""
from pathlib import Path
import struct,collections,sys,math,re
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
  name=self.apis[a];self.calls[name]+=1;v=1;high=None
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
  elif name in ('__rt_udiv','__rt_sdiv'):
   d,n=self.arg(0),self.arg(1)
   if name=='__rt_sdiv':
    d=d-(1<<32) if d>>31 else d;n=n-(1<<32) if n>>31 else n
   v=abs(n)//abs(d) if d else 0
   if (n<0)!=(d<0):v=-v
   high=(n-v*d)&0xffffffff if d else 0;v&=0xffffffff
  elif name in ('__ll_mul','__rt_udiv64by64','__rt_sdiv64by64'):
   x=self.arg(0)|(self.arg(1)<<32);y=self.arg(2)|(self.arg(3)<<32)
   if name=='__ll_mul':value=x*y
   else:
    if name=='__rt_sdiv64by64':x=x-(1<<64) if x>>63 else x;y=y-(1<<64) if y>>63 else y
    value=abs(x)//abs(y) if y else 0
    if (x<0)!=(y<0):value=-value
   value&=(1<<64)-1;v=value&0xffffffff;high=value>>32
  elif name in ('sqrtf','__itos','__stoi','__stod','__dtos','sqrt','__addd','__muld','__divd'):
   f32=lambda n:struct.unpack('<f',struct.pack('<I',n))[0]
   f64=lambda n:struct.unpack('<d',struct.pack('<Q',n))[0]
   bits32=lambda x:struct.unpack('<I',struct.pack('<f',x))[0]
   bits64=lambda x:struct.unpack('<Q',struct.pack('<d',x))[0]
   a=self.arg(0);wide=a|(self.arg(1)<<32)
   if name=='sqrtf':v=bits32(math.sqrt(f32(a)))
   elif name=='__itos':v=bits32(a-(1<<32) if a>>31 else a)
   elif name=='__stoi':v=int(f32(a))&0xffffffff
   elif name=='__dtos':v=bits32(f64(wide))
   else:
    if name=='__stod':value=bits64(f32(a))
    elif name=='sqrt':value=bits64(math.sqrt(f64(wide)))
    else:
     x=f64(wide);y=f64(self.arg(2)|(self.arg(3)<<32));value=bits64(x+y if name=='__addd' else x*y if name=='__muld' else x/y)
    v=value&0xffffffff;high=value>>32
  elif name in ('__adds','__muls','__divs'):
   f=lambda x:struct.unpack('<f',struct.pack('<I',x))[0];x,y=f(self.arg(0)),f(self.arg(1));v=struct.unpack('<I',struct.pack('<f',x+y if name=='__adds' else x*y if name=='__muls' else x/y))[0]
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
  if high is not None:u.reg_write(UC_ARM_REG_R1,high)
  u.reg_write(UC_ARM_REG_R0,v);u.reg_write(UC_ARM_REG_PC,u.reg_read(UC_ARM_REG_LR))
 def run(self):
  self.u.mem_write(0x7000d000,self.command.encode('utf-16le')+b'\0\0')
  for r,v in zip(R,[0x10000,0,0x7000d000,1]):self.u.reg_write(r,v)
  self.u.emu_start(self.pe.OPTIONAL_HEADER.ImageBase+self.pe.OPTIONAL_HEADER.AddressOfEntryPoint,0,timeout=20000000)
ROOT=Path(__file__).resolve().parents[1]
root=ROOT/'build/card/GZHD999989'
class BenchmarkGuest(Guest):
 def hook(self,u,a,size,data):
  if self.apis.get(a)=='GetTickCount':
   self.tick_count=getattr(self,'tick_count',0)+1
   step=250 if self.tick_count<=5 else 200 if self.tick_count<=11 else 333
   self.time+=step;u.reg_write(UC_ARM_REG_R0,self.time);u.reg_write(UC_ARM_REG_PC,u.reg_read(UC_ARM_REG_LR));return
  super().hook(u,a,size,data)
for mode in ['integer','floating']:
 g=BenchmarkGuest(root/'AUTORUN.EXE');g.command='--hd-'+mode;g.run();s=bytes(g.files['\\Flash Disk\\'+('INTEGER.TXT' if mode=='integer' else 'FLOATING.TXT')]).decode();assert 'BENCHMARK_RESULT PASS' in s and g.exit==0,s;assert len(re.findall(r'^elapsed_ms=',s,re.M))==3;
 import re
 rates=list(map(int,re.findall(r'^work_iterations_per_second=(\d+)',s,re.M)));score=int(re.search(r'^FINAL_[A-Z]+_SCORE_ITERATIONS_PER_SECOND=(\d+)',s,re.M)[1]);assert len(set(rates))==3 and score==sorted(rates)[1];assert not g.created;print(mode, 'integrated ARM benchmark: 3 passes + warmup PASS; no child process')
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

# GPS/COM8 protocol checks use a controlled provider, independent sentence
# checksums, fragmented reads, and corrupt/no-fix variants. No real GPS claim.
class GpsGuest(Guest):
 def __init__(self,path,fix=True,corrupt=False):
  super().__init__(path);self.ports={};self.fix=fix;self.corrupt=corrupt
  self.files['\\Flash Disk\\HARDWARELIVE.TXT']=bytearray(b' '*240)
  self.live_sizes=[]
 def hook(self,u,a,size,data):
  name=self.apis.get(a);h=self.arg(0);v=1;handled=True
  if name=='CreateFileW' and self.wide(h) in ('GPS1:','COM8:'):
   kind=self.wide(h)
   if any(p['kind']==kind for p in self.ports.values()):self.error=32;v=0xffffffff
   else:
    v=0xdead1000+len(self.ports);self.ports[v]={'kind':kind,'queue':None,'dcb':bytearray(28),'timeouts':bytes(20)};p=self.ports[v];struct.pack_into('<II',p['dcb'],0,28,4800);p['dcb'][8]=1;p['dcb'][18]=8
  elif name=='CloseHandle' and h in self.ports:self.ports.pop(h)
  elif name in ('ReadFile','WriteFile') and h in self.ports:
   p=self.ports[h];ptr=self.arg(1);n=self.arg(2);done=self.arg(3)
   if name=='WriteFile':self.error=5;v=0;self.put(done,0) if done else None
   elif not ptr or (p['kind']=='GPS1:' and n!=180):self.error=87;v=0;self.put(done,0) if done else None
   elif p['kind']=='GPS1:':
    packet=bytearray(180);struct.pack_into('<I',packet,15,int(self.fix));struct.pack_into('<i',packet,21,510000000);struct.pack_into('<i',packet,25,10000000);struct.pack_into('<I',packet,37,100);u.mem_write(ptr,bytes(packet));self.put(done,180)
   else:
    if p['queue'] is None:
     def sentence(body):
      checksum=0
      for c in body.encode():checksum^=c
      return f'${body}*{checksum:02X}\r\n'.encode()
     status='A' if self.fix else 'V';quality='1' if self.fix else '0'
     stream=sentence('GPGGA,120000.00,5100.000000,N,00100.000000,E,'+quality+',00,,0.00,M,,M,,')+sentence('GPRMC,120000.00,'+status+',5100.000000,N,00100.000000,E,0.00,0.00,011126,,')
     if self.corrupt:stream=stream.replace(b'*',b'*0',1)
     p['queue']=bytearray(stream)
    chunk=bytes(p['queue'][:n]);del p['queue'][:n];u.mem_write(ptr,chunk) if chunk else None;self.put(done,len(chunk))
  elif name in ('GetCommState','SetCommState','GetCommTimeouts','SetCommTimeouts'):
   p=self.ports[h];key='dcb' if name.endswith('State') else 'timeouts';n=28 if key=='dcb' else 20
   if name.startswith('Get'):u.mem_write(self.arg(1),bytes(p[key]))
   else:p[key]=bytes(u.mem_read(self.arg(1),n))
  elif name in ('SetupComm','PurgeComm'):pass
  elif name=='ClearCommError':self.put(self.arg(1),0);u.mem_write(self.arg(2),bytes(12))
  else:handled=False
  if handled:u.reg_write(UC_ARM_REG_R0,v);u.reg_write(UC_ARM_REG_PC,u.reg_read(UC_ARM_REG_LR))
  else:
   if name=='WriteFile' and h in self.handles and self.handles[h][0]=='\\Flash Disk\\HARDWARELIVE.TXT':
    # The mailbox must remain writable even with ZERO free quota: every write
    # overwrites its preallocated bytes, never appends or extends the file.
    assert self.handles[h][1]==0 and self.arg(2)==240
    self.live_sizes.append(len(self.files['\\Flash Disk\\HARDWARELIVE.TXT']))
   super().hook(u,a,size,data)
for fix,corrupt in [(True,False),(False,False),(True,True)]:
 g=GpsGuest(root/'AUTORUN.EXE',fix,corrupt);g.u.emu_start(symbols['diag_gps_entry'],0,timeout=20000000)
 report=bytes(g.files['\\Flash Disk\\GPSTEST.TXT']).decode();assert ('GPSTEST_RESULT FAIL' if corrupt else 'GPSTEST_RESULT PASS') in report,report
 assert not g.ports and set(g.live_sizes)=={240};assert len(g.files['\\Flash Disk\\HARDWARELIVE.TXT'])==240
 if not fix:assert 'COM8_POSITION NO_FIX' in report
print('integrated ARM GPS: GPS1 + COM8 8N1, fragmented NMEA, checksum rejection, NO_FIX, cleanup PASS')
print('live reporting: repeated ARM writes never grow the 240-byte preallocated mailbox PASS')
