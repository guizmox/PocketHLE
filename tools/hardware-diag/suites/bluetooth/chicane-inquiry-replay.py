"""Replay Chicane's real ARM inquiry loop with controlled API responses.

Usage: python chicane-inquiry-replay.py path/to/Autorun.exe
Requires pefile and unicorn; no game code is bundled. This verifies the guest
branch, not physical Bluetooth. Rust tests exercise the actual HLE dispatcher.
"""
import struct
import sys
from pathlib import Path

# Reuse executable loading and import-thunk discovery, without running its test.
exec(Path(__file__).with_name("chicane-init-replay.py").read_text().split("def run(")[0])


def replay(shrink_capacity):
    u = Uc(UC_ARCH_ARM, UC_MODE_ARM)
    image = p.get_memory_mapped_image()
    u.mem_map(base, ((len(image) + 4095) // 4096) * 4096)
    u.mem_write(base, image)
    u.mem_map(0x50000000, 0x20000)
    u.mem_map(0x60000000, 0x10000)
    state = 0x50000000
    u.mem_write(state, struct.pack("<I", 15))
    u.mem_write(state + 0x8c20, struct.pack("<I", 1))
    u.reg_write(UC_ARM_REG_R0, state)
    u.reg_write(UC_ARM_REG_SP, 0x6000f000)
    u.reg_write(UC_ARM_REG_LR, 0x60010000)
    names = ["PLT_K100", "BlackBerry KEY2", "A much longer device name"]
    index = 0
    attempts = []
    hosts = []

    def word(addr):
        return struct.unpack("<I", u.mem_read(addr, 4))[0]

    def ret(value):
        u.reg_write(UC_ARM_REG_R0, value & 0xffffffff)
        u.reg_write(UC_ARM_REG_PC, u.reg_read(UC_ARM_REG_LR))

    def hook(_, pc, size, userdata):
        nonlocal index
        r = [u.reg_read(reg) for reg in [UC_ARM_REG_R0, UC_ARM_REG_R1, UC_ARM_REG_R2, UC_ARM_REG_R3]]
        if pc in [0x67af8, 0x67ae8, 0x2e8f4, 0x2ec6c, 0x2e968, 0x67b08]:
            ret(0)
        elif pc == 0x678f8:
            u.mem_write(r[0], bytes(u.mem_read(r[1], r[2])))
            ret(r[0])
        elif pc == 0x2eb0c:
            address = word(r[1] + 8)
            attempts.append(address)
            ret(0 if address == 1 else 1)
        elif pc == 0x2d134:
            hosts.append(index)
            ret(0)
        elif pc in thunks:
            _, name = thunks[pc]
            if name == "WSALookupServiceNextW":
                if index == len(names):
                    ret(-1)
                    return
                required = 124 + len((names[index] + "\0").encode("utf-16le"))
                capacity = word(r[2])
                if shrink_capacity or capacity < required:
                    u.mem_write(r[2], struct.pack("<I", required))
                if capacity < required:
                    ret(-1)
                    return
                result = bytearray(required)
                for off, value in [(0, 60), (4, r[3]+124), (44, 1), (48, r[3]+60), (68, r[3]+84), (72, 40)]:
                    struct.pack_into("<I", result, off, value)
                struct.pack_into("<H", result, 84, 32)
                struct.pack_into("<Q", result, 92, index+1)
                result[124:] = (names[index] + "\0").encode("utf-16le")
                u.mem_write(r[3], bytes(result))
                index += 1
                ret(0)
            elif name in ["WSAGetLastError", "WSALookupServiceEnd"]:
                ret(0)
            else:
                raise RuntimeError((hex(pc), name))

    u.hook_add(UC_HOOK_CODE, hook)
    u.emu_start(0x2cf58, 0x60010000, count=20000)
    print("Shrink capacity:", shrink_capacity, "connection attempts:", attempts, "hosts:", hosts)
    assert attempts == ([1] if shrink_capacity else [1, 2, 3])
    assert hosts == ([] if shrink_capacity else [2, 3])


replay(True)
replay(False)
