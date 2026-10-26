"""Diagnostic only: execute the original MSR ARM pixel conversion, not a patched game.
Requires pefile, unicorn, Pillow; run with the CAB's extracted MSR executable path.
"""
import sys, struct
from pathlib import Path
import pefile
from unicorn import Uc, UC_ARCH_ARM, UC_MODE_ARM
from unicorn.arm_const import UC_ARM_REG_SP, UC_ARM_REG_R5
exe=Path(sys.argv[1])
p=pefile.PE(str(exe)); image=p.get_memory_mapped_image();base=p.OPTIONAL_HEADER.ImageBase
assert image[0x1de48-base:0x1de50-base]==bytes.fromhex('ff3ca0e3b000d5e1'), 'Unexpected executable: refuse address-based diagnostic'
u=Uc(UC_ARCH_ARM,UC_MODE_ARM);u.mem_map(base,(len(image)+4095)&~4095);u.mem_write(base,image);u.mem_map(0x10000000,0x2000)
def guest_channels(pixel):
 u.mem_write(0x10001000,struct.pack('<H',pixel));u.reg_write(UC_ARM_REG_SP,0x10000800);u.reg_write(UC_ARM_REG_R5,0x10001000)
 u.emu_start(0x1de48,0x1de74,count=40)
 # guest stores red at sp+0x14, green at +0xc, blue at +0x10
 return tuple(struct.unpack('<I',u.mem_read(0x10000800+off,4))[0] for off in [0x14,0xc,0x10])
old=guest_channels(0xffff);fixed=guest_channels(0x7fff)
print('Original ARM conversion: RGB565 white ->',old,'; RGB555 white ->',fixed)
assert old[0]==63 and fixed==(31,31,31)
for r in range(0,256,17):
 for g in range(0,256,17):
  for b in range(0,256,17):
   pixel=((r>>3)<<10)|((g>>3)<<5)|(b>>3)
   assert guest_channels(pixel)==(r>>3,g>>3,b>>3)
print('4096 actual ARM colour conversions passed; every RGB555 channel remains inside its 32-entry table.')
