"""Independent IEEE/unsigned arithmetic oracle for the fixed v2 workloads.

This is a host reference, not a performance measurement. No emulator imports.
"""
import math
import struct

MASK32 = (1 << 32) - 1
MASK64 = (1 << 64) - 1
U32_INPUTS = [0x7fffffff, 0x80000001, 0xffffffff, 0x12345678,
              0x01020304, 0xdeadbeef, 0x00010000, 0xfedcba98]
DIVISORS = [3, 7, 31, 65537, 257, 11, 17, 65521]
U64_INPUTS = [0x123456789abcdef0, 0x8000000100000001,
              0x7ffffffffffffffe, 0xffffffffffffff01, 0x0000000200000001,
              0x0000ffff12340000, 0xfedcba9876543210, 0x00000001ffffffff]
DIVISORS64 = [3, 0x100000001, 65537, 17, 0x100000003, 257, 65521, 0x10001]
FLOAT_INPUTS = [0., -1.5, 3.25, -17., 123.5, -.0625, 1024., -63.75]
DOUBLE_INPUTS = [1e100, -1e-100, 0., -3.25, 123456789012.25, 1e-50, -1e50, 63.75]
INT_CASES = ['u32_multiply_add', 'u32_divide_remainder', 'i32_divide_remainder',
             'u64_add_shift', 'u64_multiply_runtime', 'u64_divide_runtime', 'i64_divide_runtime']
FLOAT_CASES = ['f32_add_runtime', 'f32_multiply_runtime', 'f32_divide_runtime',
               'f32_sqrt_runtime', 'i32_f32_roundtrip_runtime', 'f32_f64_roundtrip_runtime',
               'f64_add_multiply_runtime', 'f64_divide_runtime', 'f64_sqrt_runtime']

def f32bits(x):
    return struct.unpack('<I', struct.pack('<f', x))[0]

def f32(x):
    return struct.unpack('<f', struct.pack('<f', x))[0]

def f64bits(x):
    return struct.unpack('<Q', struct.pack('<d', x))[0]

def signed(x, bits):
    return x - (1 << bits) if x >> (bits - 1) else x

def trunc_div(n, d):
    q = abs(n) // abs(d)
    return -q if (n < 0) != (d < 0) else q

def result(name, j):
    a, d = U32_INPUTS[j], DIVISORS[j]
    b, e = U64_INPUTS[j], DIVISORS64[j]
    x = FLOAT_INPUTS[j]
    if name == 'u32_multiply_add':
        return (a * 1664525 + 1013904223) & MASK32
    if name == 'u32_divide_remainder':
        return (a // d) | ((a % d) << 32)
    if name == 'i32_divide_remainder':
        n = signed(a, 32)
        divisor = -d if j & 1 else d
        q = trunc_div(n, divisor)
        return (q & MASK32) | (((n - q * divisor) & MASK32) << 32)
    if name == 'u64_add_shift':
        return (((b << 7) ^ (b >> 13)) + e) & MASK64
    if name == 'u64_multiply_runtime':
        return b * e & MASK64
    if name == 'u64_divide_runtime':
        return b // e
    if name == 'i64_divide_runtime':
        return trunc_div(signed(b, 64), -e if j & 1 else e) & MASK64
    if name == 'f32_add_runtime':
        return f32bits(x + 1.25)
    if name == 'f32_multiply_runtime':
        return f32bits(x * .75)
    if name == 'f32_divide_runtime':
        return f32bits(x / 3.25)
    if name == 'f32_sqrt_runtime':
        return f32bits(math.sqrt(abs(x)))
    if name == 'i32_f32_roundtrip_runtime':
        n = signed(a, 32) // 16  # ARM arithmetic right shift, valid i32 range after rounding.
        return int(f32(n)) & MASK32
    if name == 'f32_f64_roundtrip_runtime':
        return f32bits(x)
    if name == 'f64_add_multiply_runtime':
        return f64bits((DOUBLE_INPUTS[j] + 1.25) * .75)
    if name == 'f64_divide_runtime':
        return f64bits(DOUBLE_INPUTS[j] / 3.25)
    if name == 'f64_sqrt_runtime':
        return f64bits(math.sqrt(abs(DOUBLE_INPUTS[j])))
    raise ValueError(name)

def checksum(name):
    h = 0x811c9dc5
    for i in range(1024):
        value = result(name, i & 7)
        h = ((h ^ (value & MASK32)) * 16777619) & MASK32
        h = ((h ^ (value >> 32)) * 16777619) & MASK32
    return h

if __name__ == '__main__':
    for name in INT_CASES + FLOAT_CASES:
        print(name, f'0x{checksum(name):08x}u')
