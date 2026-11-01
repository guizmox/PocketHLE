# Replay Chicane's ARM Bluetooth initializer with controlled API responses.
# Requires pefile + unicorn. Guest executable is supplied by the user, never bundled.
# This isolates the game branch; Rust tests validate the corresponding real HLE APIs.
import pefile, struct, sys
if len(sys.argv) != 2:
    raise SystemExit("Usage: python chicane-init-replay.py path/to/Autorun.exe")
from unicorn import Uc, UC_ARCH_ARM, UC_MODE_ARM, UC_HOOK_CODE
from unicorn.arm_const import *
p = pefile.PE(sys.argv[1])
imports={s.address:(imp.dll.decode(),s.name.decode() if s.name else str(s.ordinal)) for imp in p.DIRECTORY_ENTRY_IMPORT for s in imp.imports}
sec=p.sections[0];data=sec.get_data();base=p.OPTIONAL_HEADER.ImageBase
thunks={}
for off in range(0,len(data)-16,4):
 a,b,c,iat=struct.unpack_from('<4I',data,off)
 if (a,b,c)==(0xe59fc004,0xe59cc000,0xe12fff1c) and iat in imports:
  thunks[base+sec.VirtualAddress+off]=imports[iat]
def run(driver_available):
 u=Uc(UC_ARCH_ARM,UC_MODE_ARM); image=p.get_memory_mapped_image()
 u.mem_map(base,((len(image)+4095)//4096)*4096);u.mem_write(base,image)
 u.mem_map(0x50000000,0x20000);u.mem_map(0x60000000,0x10000)
 state=0x50000000; u.mem_write(state+0xc,struct.pack('<I',0xffffffff));u.mem_write(state+0x14,b'Unknown\0')
 u.reg_write(UC_ARM_REG_R0,state);u.reg_write(UC_ARM_REG_SP,0x6000f000);u.reg_write(UC_ARM_REG_LR,0x60010000)
 trace=[]
 def word(addr):return struct.unpack('<I',u.mem_read(addr,4))[0]
 def ret(v):u.reg_write(UC_ARM_REG_R0,v&0xffffffff);u.reg_write(UC_ARM_REG_PC,u.reg_read(UC_ARM_REG_LR))
 def hook(_,pc,size,userdata):
  if pc==0x2d36c:trace.append(('failure_cleanup',));ret(0);return
  if pc not in thunks:return
  dll,name=thunks[pc];r=[u.reg_read(x) for x in [UC_ARM_REG_R0,UC_ARM_REG_R1,UC_ARM_REG_R2,UC_ARM_REG_R3]]
  sp=u.reg_read(UC_ARM_REG_SP)
  trace.append((name,*[hex(v) for v in r]))
  if name=='WSAStartup':ret(0)
  elif name=='168': # CreateFileW
   ret(0xb70001 if driver_available else 0xffffffff)
  elif name=='179':
   assert r[1]==0x01012120
   assert r[2:]==[0,0] and word(sp)==0 and word(sp+4)==0
   u.mem_write(word(sp+8),bytes(4));ret(1 if driver_available else 0)
  elif name=='gethostname':u.mem_write(r[0],b'Android test phone\0');ret(0)
  else:raise RuntimeError((hex(pc),dll,name,r))
 u.hook_add(UC_HOOK_CODE,hook)
 u.emu_start(0x2d264,0x60010000,count=10000)
 name=bytes(u.mem_read(state+0x14,64)).split(b'\0')[0].decode()
 result=u.reg_read(UC_ARM_REG_R0)
 print('BTH1 available:',driver_available,'initializer:',result,'contestant:',name,'trace:',trace)
 assert (result,name)==((1,'Android test phone') if driver_available else (0,'Unknown'))
run(False);run(True)
