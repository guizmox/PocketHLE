//! Native ARM/VFP thunks for pure-arithmetic and pure-memory CRT
//! helpers.
//!
//! Pocket PC binaries are built with the soft-float ABI, so any
//! floating-point op turns into a call to a `coredll.dll` helper
//! (`__adds`, `__divs`, `__stoi`, …). Likewise the C runtime ships
//! `memcpy` / `memset` / `strlen` / … as separate exports rather
//! than inlining them. On real hardware those helpers are tiny
//! hand-written routines; under HLE we used to route every call
//! through the Rust dispatcher (one `emu_stop` → handler →
//! `emu_start` per call). For a game like Derby that calls
//! `memcpy` ~1 100 times per second that overhead alone is a
//! double-digit percentage of the wall-clock budget.
//!
//! This module provides native ARM/VFP machine code that performs
//! the same operation in a single JIT-translated basic block. The
//! `pocket-cpu::UnicornCpu` boot path enables VFP for us
//! (FPEXC.EN=1, CPACR.CP10/11), so VFP instructions become a few
//! native SSE/AVX ops on the host. Hardware integer divide
//! (`SDIV`/`UDIV`) is supported on Unicorn's default ARMv7-A model.
//!
//! Each thunk is exactly [`THUNK_STRIDE`](super::THUNK_STRIDE) bytes
//! (32) — every slot in the synthetic IAT region is padded to that
//! stride. Unused slots are filled with `bx lr` so an accidental
//! fall-through returns harmlessly.
//!
//! ABI notes (matches the Microsoft / RVCT soft-float runtime and
//! AAPCS for everything else):
//! * f32 ops take `a` in r0 (and `b` in r1 for binary), return in r0;
//! * f64 ops take `a` packed in (r0, r1) (lo word first) and `b` in
//!   (r2, r3); the result is returned in (r0, r1);
//! * `__rt_sdiv` / `__rt_udiv` take `divisor` in r0 and `dividend`
//!   in r1, returning the quotient in r0 and the remainder in r1;
//! * `memcpy` / `memset` / `strcpy` / `wcscpy` return their
//!   destination pointer (r0 unchanged across the call);
//! * `strlen` / `wcslen` / `lstrlen*` return the length in r0;
//! * `strcmp` / `lstrcmpA` / `wcscmp` / `lstrcmpW` return signed
//!   `(*a) - (*b)` at the first difference;
//! * `GetTickCount` returns the current tick (ms since the first
//!   call within the host process) by reading the host-managed
//!   cell at [`TICK_PAGE_VA`]; the run loop refreshes that cell
//!   between every emulator slice via
//!   [`refresh_tick_page`].
//!
//! Note that `lstrlenA` / `lstrcpyA` etc. are byte-exact aliases of
//! their `str*` counterparts on Pocket PC — they're entry points
//! into the same code in `coredll.dll` — so we share the assembled
//! image rather than copy-pasting it.

/// `bx lr` (ARM, little endian). Used both as the trailing return
/// instruction of every thunk and as harmless padding for the
/// otherwise-unused tail of each 32-byte slot.
const BX_LR: u32 = 0xE12F_FF1E;

/// Pure 64-bit helpers cannot fit a general integer divide in a 32-byte ARM
/// thunk. Colors' pose interpolation calls these repeatedly per model. A CPU
/// backend may serve their exact register arithmetic without leaving its run
/// loop; unsupported backends retain the regular handler.
pub fn install_inline_arithmetic64(
    cpu: &mut dyn pocket_cpu::Cpu,
    va: u32,
    dll: &str,
    name: &str,
) -> Result<bool, pocket_cpu::CpuError> {
    use pocket_cpu::arithmetic64::Arithmetic64;
    if cpu.arch() != pocket_cpu::Arch::Arm || !dll.eq_ignore_ascii_case("coredll.dll") {
        return Ok(false);
    }
    let operation = match name {
        "__i64tod" => Arithmetic64::SignedToDouble,
        "__u64tod" => Arithmetic64::UnsignedToDouble,
        "__dtoi64" => Arithmetic64::DoubleToSigned,
        "__dtou64" => Arithmetic64::DoubleToUnsigned,
        "__rt_sdiv64by64" => Arithmetic64::SignedDivide,
        "__rt_udiv64by64" => Arithmetic64::UnsignedDivide,
        "__rt_srem64by64" => Arithmetic64::SignedRemainder,
        "__rt_urem64by64" => Arithmetic64::UnsignedRemainder,
        _ => return Ok(false),
    };
    cpu.write_mem(va, &thunk_bytes(&pad(&[BX_LR])))?;
    cpu.install_inline_arithmetic64(va, operation)
}

/// Build an 8-word thunk image, padding with `bx lr` after the
/// supplied instructions.
const fn pad(words: &[u32]) -> [u32; 8] {
    let mut out = [BX_LR; 8];
    let mut i = 0;
    while i < words.len() {
        out[i] = words[i];
        i += 1;
    }
    out
}

/// Base address of the host-managed "tick page" — a single 4 KiB
/// guest-readable page whose first u32 always holds the current
/// `GetTickCount` result. Updated by the run loop's frame hook so
/// guest code can read it with a plain `LDR`. Chosen just below
/// [`super::THUNK_REGION_BASE`] so it sits inside the same logical
/// "synthetic kernel" address neighbourhood.
pub const TICK_PAGE_VA: u32 = 0x6FFF_F000;
/// Offset within [`TICK_PAGE_VA`] of the live tick value (u32 LE).
pub const TICK_VALUE_OFFSET: u32 = 0;

// Bulk copies remain host-vectorised. A one-byte copy instead runs directly
// in the guest: byte-at-a-time stream reads otherwise stop/restart the CPU
// millions of times. The other sizes branch to a hooked tail in the same slot.
const BYTE_COPY: [u32; 8] = [
    0xE352_0001, // cmp r2, #1
    0x1A00_0004, // bne slot+28 (host fallback)
    0xE5D1_3000, // ldrb r3, [r1]
    0xE5C0_3000, // strb r3, [r0]
    BX_LR,
    BX_LR,
    BX_LR,
    BX_LR, // hooked fallback, preserving all original arguments
];

/// Shared bounded-copy helper in the synthetic tick page or a DLL thunk pool.
/// Aligned 4/16-byte copies load all source words before storing. Other small
/// copies choose the direction needed for memmove, without touching neighbours.
pub const SMALL_COPY_HELPER_BYTES: u32 = 512;
pub const SMALL_COPY_HELPER: u32 = TICK_PAGE_VA + 0x100;
pub fn install_small_copy_helper(
    cpu: &mut dyn pocket_cpu::Cpu,
) -> Result<(), pocket_cpu::CpuError> {
    install_small_copy_helper_at(cpu, SMALL_COPY_HELPER)
}
pub fn install_small_copy_helper_at(
    cpu: &mut dyn pocket_cpu::Cpu,
    address: u32,
) -> Result<(), pocket_cpu::CpuError> {
    let words: [u32; 34] = [
        0xe3520004, // cmp r2,#4
        0x1a000005, // b sixteen
        0xe180c001, // orr ip,r0,r1
        0xe31c0003, // tst ip,#3
        0x1a00000a, // b generic
        0xe5913000, // ldr r3,[r1]
        0xe5803000, // str r3,[r0]
        0xe12fff1e, // bx lr
        0xe3520010, // cmp r2,#16
        0x1a000005, // b generic
        0xe180c001, // orr ip,r0,r1
        0xe31c0003, // tst ip,#3
        0x1a000002, // b generic
        0xe891100e, // ldmia r1,{r1,r2,r3,ip}
        0xe880100e, // stmia r0,{r1,r2,r3,ip}
        0xe12fff1e, // bx lr
        0xe3520000, // cmp r2,#0
        0x012fff1e, // bxeq lr
        0xe1500001, // cmp r0,r1
        0x012fff1e, // bxeq lr
        0x3a000006, // b forward
        0xe0811002, // add r1,r1,r2
        0xe080c002, // add ip,r0,r2
        0xe5713001, // ldrb r3,[r1,#-1]!
        0xe56c3001, // strb r3,[ip,#-1]!
        0xe2522001, // subs r2,r2,#1
        0x1afffffb, // b backward
        0xe12fff1e, // bx lr
        0xe1a0c000, // mov ip,r0
        0xe4d13001, // ldrb r3,[r1],#1
        0xe4cc3001, // strb r3,[ip],#1
        0xe2522001, // subs r2,r2,#1
        0x1afffffb, // b forward_loop
        0xe12fff1e, // bx lr
    ];
    let bytes: Vec<_> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    cpu.write_mem(address, &bytes)?;
    install_int64_double(cpu, address + 0x100, true)?;
    install_int64_double(cpu, address + 0x180, false)
}

// high32*2^32 and low32 are exactly representable doubles. Only their sum
// rounds, so the result matches an integer->double cast even beyond 2^53.
// Use RN temporarily and restore the full FPSCR (rounding, exception flags).
fn install_int64_double(
    cpu: &mut dyn pocket_cpu::Cpu,
    address: u32,
    signed: bool,
) -> Result<(), pocket_cpu::CpuError> {
    let words: [u32; 18] = [
        0xeef1ca10,                                   // vmrs ip,fpscr
        0xe3cc38c0,                                   // bic r3,ip,#0xc00000
        0xe3c33c9f,                                   // bic r3,r3,#0x9f00
        0xeee13a10,                                   // vmsr fpscr,r3
        0xee000a10,                                   // vmov s0,r0
        0xeeb80b40,                                   // vcvt.f64.u32 d0,s0
        0xee011a10,                                   // vmov s2,r1
        if signed { 0xeeb81bc1 } else { 0xeeb81b41 }, // vcvt d1,s2
        0xed9f2b06,                                   // vldr d2,[pc,#24]
        0xee211b02,                                   // vmul.f64 d1,d1,d2
        0xee300b01,                                   // vadd.f64 d0,d0,d1
        0xec510b10,                                   // vmov r0,r1,d0
        0xeee1ca10,                                   // vmsr fpscr,ip
        BX_LR,
        BX_LR,
        BX_LR,
        0,
        0x41f00000, // double 2^32
    ];
    let bytes: Vec<_> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
    cpu.write_mem(address, &bytes)
}

/// Address-aware small-copy thunk, shared by static imports, runtime DLLs and
/// GetProcAddress. Keep the hooked tail at +28 with original arguments intact.
pub fn native_thunk_at(dll: &str, name: &str, address: u32) -> Option<[u32; 8]> {
    native_thunk_with_helper(dll, name, address, SMALL_COPY_HELPER)
}
pub fn native_thunk_with_helper(
    dll: &str,
    name: &str,
    address: u32,
    helper: u32,
) -> Option<[u32; 8]> {
    if dll.eq_ignore_ascii_case("coredll.dll") && matches!(name, "__i64tod" | "__u64tod") {
        let target = helper + if name == "__i64tod" { 0x100 } else { 0x180 };
        let delta = i64::from(target) - (i64::from(address) + 8);
        if delta % 4 != 0 || !(-(1i64 << 25)..(1i64 << 25)).contains(&delta) {
            return None;
        }
        return Some(pad(&[0xea000000 | (((delta >> 2) as u32) & 0xffffff)]));
    }
    let original = native_thunk_for(dll, name)?;
    if hybrid_fallback_offset(dll, name).is_none() {
        return Some(original);
    }
    let delta = i64::from(helper) - (i64::from(address) + 24 + 8);
    if delta % 4 != 0 || !(-(1i64 << 25)..(1i64 << 25)).contains(&delta) {
        return Some(original);
    }
    Some([
        0xe352_0001, // cmp r2,#1
        0x05d1_3000, // ldrbeq r3,[r1]
        0x05c0_3000, // strbeq r3,[r0]
        0x012f_ff1e, // bxeq lr
        0xe352_0010, // cmp r2,#16
        0x8a00_0000, // bhi slot+28
        0xea00_0000 | (((delta >> 2) as u32) & 0x00ff_ffff),
        BX_LR, // host fallback hook
    ])
}

/// Native/hooked hybrid tail; the loader must hook only this address, never
/// the entire native slot. R0 stays the destination on both paths.
pub fn hybrid_fallback_offset(dll: &str, name: &str) -> Option<u32> {
    if dll.eq_ignore_ascii_case("coredll.dll") && matches!(name, "memcpy" | "memmove") {
        Some(28)
    } else {
        None
    }
}

/// Build a thunk that does `mov r0, #imm; bx lr`. Used to patch
/// IAT entries whose dispatcher handler is a pure constant-return
/// stub (`zero_returning` / `one_returning`). The JIT then executes
/// these calls inline with zero callback overhead — same shape as
/// the arithmetic CRT thunks above, just simpler.
///
/// Supports any unsigned 8-bit immediate (0..=255). For ARM, the
/// encoding `MOV r0, #imm` packs the rotated-immediate in bits 0..11
/// of `0xE3A0_0000`; with no rotation we can fit any 8-bit value
/// directly.
const fn const_return_thunk(imm: u32) -> [u32; 8] {
    // `mov r0, #imm` -- AArch32 immediate form `e3 a0 00 XX`.
    let mov_r0_imm = 0xE3A0_0000 | (imm & 0xFF);
    pad(&[mov_r0_imm, BX_LR])
}

/// Look up the native thunk for a handler that returns a fixed
/// `u32` constant in `r0` (with no other side effects). Returns
/// `None` if `value` doesn't fit in an 8-bit ARM mov immediate —
/// the caller should fall back to the regular dispatcher path in
/// that case.
pub fn constant_return_thunk(value: u32) -> Option<[u32; 8]> {
    if value > 0xFF {
        return None;
    }
    Some(const_return_thunk(value))
}

/// Hand-rolled `strlen(s)` — returns count in bytes, exclusive of NUL.
const STRLEN: [u32; 8] = [
    0xE1A0_1000, // mov r1, r0
    0xE4D1_2001, // ldrb r2, [r1], #1
    0xE352_0000, // cmp r2, #0
    0x1AFF_FFFC, // bne -4 (back to ldrb)
    0xE041_0000, // sub r0, r1, r0
    0xE240_0001, // sub r0, r0, #1
    BX_LR,
    BX_LR,
];

/// Hand-rolled `wcslen(s)` — returns count in u16 chars.
const WCSLEN: [u32; 8] = [
    0xE1A0_1000, // mov r1, r0
    0xE0D1_20B2, // ldrh r2, [r1], #2
    0xE352_0000, // cmp r2, #0
    0x1AFF_FFFC, // bne -4 (back to ldrh)
    0xE041_0000, // sub r0, r1, r0
    0xE240_0002, // sub r0, r0, #2
    0xE1A0_00A0, // lsr r0, r0, #1
    BX_LR,
];

/// Hand-rolled `strcpy(dst, src)` — copies through-and-including NUL,
/// returns the original `dst`.
const STRCPY: [u32; 8] = [
    0xE92D_4005, // push {r0, r2, lr}
    0xE4D1_2001, // ldrb r2, [r1], #1
    0xE4C0_2001, // strb r2, [r0], #1
    0xE352_0000, // cmp r2, #0
    0x1AFF_FFFB, // bne -5 (back to ldrb)
    0xE8BD_8005, // pop {r0, r2, pc}
    BX_LR,
    BX_LR,
];

/// Hand-rolled `wcscpy(dst, src)` — wide-char strcpy.
const WCSCPY: [u32; 8] = [
    0xE92D_4005, // push {r0, r2, lr}
    0xE0D1_20B2, // ldrh r2, [r1], #2
    0xE0C0_20B2, // strh r2, [r0], #2
    0xE352_0000, // cmp r2, #0
    0x1AFF_FFFB, // bne -5 (back to ldrh)
    0xE8BD_8005, // pop {r0, r2, pc}
    BX_LR,
    BX_LR,
];

/// Hand-rolled `strcmp(a, b)` — returns signed diff at first
/// mismatch, 0 if equal.
const STRCMP: [u32; 8] = [
    0xE4D0_2001, // ldrb r2, [r0], #1
    0xE4D1_3001, // ldrb r3, [r1], #1
    0xE052_C003, // subs r12, r2, r3
    0x1A00_0001, // bne +1  (skip the trailing cmp)
    0xE352_0000, // cmp r2, #0
    0x1AFF_FFF9, // bne -7 (back to first ldrb)
    0xE1A0_000C, // mov r0, r12
    BX_LR,
];

/// Hand-rolled `wcscmp(a, b)` — wide-char strcmp.
const WCSCMP: [u32; 8] = [
    0xE0D0_20B2, // ldrh r2, [r0], #2
    0xE0D1_30B2, // ldrh r3, [r1], #2
    0xE052_C003, // subs r12, r2, r3
    0x1A00_0001, // bne +1
    0xE352_0000, // cmp r2, #0
    0x1AFF_FFF9, // bne -7
    0xE1A0_000C, // mov r0, r12
    BX_LR,
];

const TOLOWER: [u32; 8] = [
    0xE350_0000,
    0x0A00_0004,
    0xE350_0041,
    0x3A00_0002,
    0xE350_005A,
    0x8A00_0000,
    0xE280_0020,
    BX_LR,
];

/// Hand-rolled `GetTickCount()` — reads the host-managed tick cell
/// at [`TICK_PAGE_VA`] + [`TICK_VALUE_OFFSET`] (= 0x6FFFF000) and
/// returns it.
///
/// The value is refreshed by the kernel run loop on every frame
/// hook, which fires after every dispatched WinCE call — so the
/// granularity matches what the dispatcher path used to provide
/// (the previous Rust handler also read host time once per call).
const GET_TICK_COUNT: [u32; 8] = [
    // movw r0, #0xF000  -- imm16 in {imm4=0xF, imm12=0x000}
    0xE30F_0000,
    // movt r0, #0x6FFF  -- imm16 in {imm4=0x6, imm12=0xFFF}
    0xE346_0FFF,
    0xE590_0000, // ldr r0, [r0]
    BX_LR,
    BX_LR,
    BX_LR,
    BX_LR,
    BX_LR,
];

/// Look up the native thunk for a `coredll.dll` symbol. Returns
/// `None` if the symbol isn't one of the helpers we can JIT directly
/// — the kernel falls back to the regular hooked dispatcher path
/// for those.
pub fn native_thunk_for(dll: &str, name: &str) -> Option<[u32; 8]> {
    if !dll.eq_ignore_ascii_case("coredll.dll") {
        return None;
    }
    Some(match name {
        "memcpy" | "memmove" => BYTE_COPY,
        // ---- 32-bit hardware integer divide -----------------------------
        // sdiv ip, r1, r0 / mls r1, ip, r0, r1 / mov r0, ip / bx lr
        "__rt_sdiv" => pad(&[0xE71C_F011, 0xE061_109C, 0xE1A0_000C, BX_LR]),
        "__rt_udiv" => pad(&[0xE73C_F011, 0xE061_109C, 0xE1A0_000C, BX_LR]),

        // ---- f32 binary arithmetic --------------------------------------
        // vmov s0,r0 / vmov s1,r1 / vOP.f32 s0,s0,s1 / vmov r0,s0 / bx lr
        "__adds" => pad(&[0xEE00_0A10, 0xEE00_1A90, 0xEE30_0A20, 0xEE10_0A10, BX_LR]),
        "__subs" => pad(&[0xEE00_0A10, 0xEE00_1A90, 0xEE30_0A60, 0xEE10_0A10, BX_LR]),
        "__muls" => pad(&[0xEE00_0A10, 0xEE00_1A90, 0xEE20_0A20, 0xEE10_0A10, BX_LR]),
        "__divs" => pad(&[0xEE00_0A10, 0xEE00_1A90, 0xEE80_0A20, 0xEE10_0A10, BX_LR]),
        "__negs" => pad(&[0xEE00_0A10, 0xEEB1_0A40, 0xEE10_0A10, BX_LR]),

        // ---- f32 conversions --------------------------------------------
        "__stoi" => pad(&[0xEE00_0A10, 0xEEBD_0AC0, 0xEE10_0A10, BX_LR]),
        "__stou" => pad(&[0xEE00_0A10, 0xEEBC_0AC0, 0xEE10_0A10, BX_LR]),
        "__itos" => pad(&[0xEE00_0A10, 0xEEB8_0AC0, 0xEE10_0A10, BX_LR]),
        "__utos" => pad(&[0xEE00_0A10, 0xEEB8_0A40, 0xEE10_0A10, BX_LR]),

        // ---- f64 binary arithmetic --------------------------------------
        // vmov d0,r0,r1 / vmov d1,r2,r3 / vOP.f64 d0,d0,d1 / vmov r0,r1,d0 / bx lr
        "__addd" => pad(&[0xEC41_0B10, 0xEC43_2B11, 0xEE30_0B01, 0xEC51_0B10, BX_LR]),
        "__subd" => pad(&[0xEC41_0B10, 0xEC43_2B11, 0xEE30_0B41, 0xEC51_0B10, BX_LR]),
        "__muld" => pad(&[0xEC41_0B10, 0xEC43_2B11, 0xEE20_0B01, 0xEC51_0B10, BX_LR]),
        "__divd" => pad(&[0xEC41_0B10, 0xEC43_2B11, 0xEE80_0B01, 0xEC51_0B10, BX_LR]),
        "__negd" => pad(&[0xEC41_0B10, 0xEEB1_0B40, 0xEC51_0B10, BX_LR]),

        // ---- f32 ↔ f64 / int conversions --------------------------------
        "__stod" => pad(&[0xEE00_0A10, 0xEEB7_0AC0, 0xEC51_0B10, BX_LR]),
        "__dtos" => pad(&[0xEC41_0B10, 0xEEB7_0BC0, 0xEE10_0A10, BX_LR]),
        "__itod" => pad(&[0xEE00_0A10, 0xEEB8_0BC0, 0xEC51_0B10, BX_LR]),
        "__utod" => pad(&[0xEE00_0A10, 0xEEB8_0B40, 0xEC51_0B10, BX_LR]),
        "__dtoi" => pad(&[0xEC41_0B10, 0xEEBD_0BC0, 0xEE10_0A10, BX_LR]),
        "__dtou" => pad(&[0xEC41_0B10, 0xEEBC_0BC0, 0xEE10_0A10, BX_LR]),

        // ---- short string helpers ----------------------------------------
        // `strlen` / `strcpy` / `strcmp` are typically called on short
        // strings (HUD labels, file paths). For inputs under ~300 bytes
        // a byte-by-byte ARM loop translated by Unicorn beats the
        // dispatcher round-trip even though the dispatcher itself
        // could call into the host's vectorised libc.
        //
        // Bulk memcpy/memmove sizes take the hooked hybrid tail above;
        // memset remains entirely host-dispatched. Large buffers retain
        // their vectorised host implementation rather than an ARM byte loop.
        "strlen" => STRLEN,
        "wcslen" | "lstrlenW" => WCSLEN,
        "lstrlenA" => STRLEN,
        "strcpy" | "lstrcpyA" => STRCPY,
        "wcscpy" | "lstrcpyW" => WCSCPY,
        "strcmp" | "lstrcmpA" => STRCMP,
        "wcscmp" | "lstrcmpW" => WCSCMP,
        "tolower" => TOLOWER,

        // ---- host-managed tick page --------------------------------------
        // Reads `*(u32*)0x6FFFF000`, which the run loop refreshes
        // before every dispatched WinCE call. See [`TICK_PAGE_VA`].
        "GetTickCount" | "timeGetTime" => GET_TICK_COUNT,

        _ => return None,
    })
}

/// Refresh the tick cell at [`TICK_PAGE_VA`] with the current
/// monotonic time in milliseconds since the first call, so the
/// [`GET_TICK_COUNT`] thunk's plain `LDR` always sees a fresh value.
///
/// `last_written` is the caller's memo of the value already in the
/// page. The run loop calls this once per slice, and a slice ends on
/// *every* emulated WinCE API call — Zuma makes ~8 500 of them per
/// displayed frame, i.e. roughly 280 per millisecond, so ~99 % of
/// those calls would re-write a byte pattern the page already holds.
/// Skipping them turns the guest-memory write into a rounding error
/// and leaves only the clock read, which is a vDSO call. Reading the
/// clock every slice is not optional: Zuma paces its own frames off
/// `GetTickCount`, so a value that advances in slice-counted jumps
/// instead of real milliseconds changes how fast the game runs.
///
/// The memo belongs to the caller rather than a `static` because it is
/// only valid for the one CPU whose page was written; a global would
/// silently skip the seeding write for a second [`crate::Process`].
///
/// The epoch is an [`Instant`], not a `SystemTime`: `GetTickCount` is
/// documented to advance monotonically, and a wall-clock adjustment
/// (NTP, a laptop resuming) that moved it backwards would strand any
/// guest sitting in a `while (GetTickCount() < deadline)` spin.
///
/// Errors are swallowed: a failing `write_mem` here would mean the
/// page wasn't mapped, which is purely a kernel bug — and the
/// previous `coredll::get_tick_count` dispatcher implementation
/// also kept running silently when its underlying clock call
/// failed, so we preserve that behaviour.
pub fn refresh_tick_page(cpu: &mut dyn crate::Cpu, last_written: &mut Option<u32>) {
    use std::sync::OnceLock;
    use std::time::Instant;
    static START: OnceLock<Instant> = OnceLock::new();
    let delta = START.get_or_init(Instant::now).elapsed().as_millis() as u32;
    if *last_written == Some(delta) {
        return;
    }
    *last_written = Some(delta);
    let _ = cpu.write_mem(TICK_PAGE_VA + TICK_VALUE_OFFSET, &delta.to_le_bytes());
}

/// Convert an instruction array into a flat little-endian byte
/// buffer suitable for `cpu.write_mem`.
pub fn thunk_bytes(words: &[u32; 8]) -> [u8; 32] {
    let mut out = [0u8; 32];
    for (i, w) in words.iter().enumerate() {
        let bytes = w.to_le_bytes();
        out[i * 4] = bytes[0];
        out[i * 4 + 1] = bytes[1];
        out[i * 4 + 2] = bytes[2];
        out[i * 4 + 3] = bytes[3];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constant_return_thunk_emits_mov_bx_lr() {
        let words = constant_return_thunk(0).expect("0 is encodable");
        // First instruction: `mov r0, #0`.
        assert_eq!(words[0], 0xE3A0_0000);
        // Second instruction: `bx lr`.
        assert_eq!(words[1], 0xE12F_FF1E);
        // Trailing slots are `bx lr` padding.
        for w in &words[2..] {
            assert_eq!(*w, 0xE12F_FF1E);
        }
    }

    #[test]
    fn constant_return_thunk_for_one_returns_imm1() {
        let words = constant_return_thunk(1).expect("1 is encodable");
        assert_eq!(words[0], 0xE3A0_0001);
        assert_eq!(words[1], 0xE12F_FF1E);
    }

    #[test]
    fn constant_return_thunk_for_max_imm8() {
        let words = constant_return_thunk(0xFF).expect("0xFF is encodable");
        assert_eq!(words[0], 0xE3A0_00FF);
    }

    #[test]
    fn constant_return_thunk_rejects_unencodable() {
        // 0x100 doesn't fit in the unrotated ARM 8-bit mov-immediate
        // slot — the caller should fall back to the dispatcher.
        assert!(constant_return_thunk(0x100).is_none());
        assert!(constant_return_thunk(0xFFFF_FFFF).is_none());
    }
}
