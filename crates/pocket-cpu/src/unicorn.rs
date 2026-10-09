//! `unicorn-engine`-backed CPU.
//!
//! Compiled only with `--features unicorn`. Apart from the build cost,
//! this is the authoritative ARM backend used at runtime.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::OnceLock;

use ::unicorn_engine::unicorn_const::{
    Arch as UcArch, MemType, Mode, Prot as UcProt, TlbEntry, TlbType,
};
use ::unicorn_engine::{RegisterARM, RegisterMIPS, Unicorn, UcHookId};

use crate::{regs::ArmReg, Arch, Cpu, CpuError, Prot, StopReason};

/// Which guest pages exist, and which of them the CPU is allowed to
/// fetch instructions from.
///
/// Shared with the virtual-TLB fill hook installed by
/// [`UnicornCpu::new_for_arch`], which is the only thing that answers
/// "may this page be read / written / executed" once we take Unicorn
/// off its architectural page-table walk.
#[derive(Default)]
struct GuestMap {
    /// `(start, end_exclusive, executable)` for every successful
    /// [`Cpu::map_region`], kept sorted by `start`.
    regions: Vec<(u64, u64, bool)>,
    /// Pages promoted to executable at run time because the guest was
    /// caught fetching from them even though their mapping is not
    /// executable, kept sorted.
    promoted_exec: Vec<u64>,
    /// Explicit VirtualAlloc permissions take precedence over compatibility
    /// promotion of old heap/code arenas. Latest range overrides earlier ones.
    protected: Vec<(u64, u64, Prot)>,
}

impl GuestMap {
    fn insert(&mut self, start: u64, len: u64, exec: bool) {
        let entry = (start, start.saturating_add(len), exec);
        let at = self.regions.partition_point(|r| r.0 < start);
        self.regions.insert(at, entry);
    }

    fn remove(&mut self, start: u64, len: u64) {
        let end = start + len;
        self.regions = self.regions.iter().flat_map(|&(lo, hi, exec)| {
            let mut parts = Vec::new();
            if hi <= start || lo >= end { parts.push((lo, hi, exec)); }
            else {
                if lo < start { parts.push((lo, start, exec)); }
                if hi > end { parts.push((end, hi, exec)); }
            }
            parts
        }).collect();
        self.protected = self.protected.iter().flat_map(|&(lo, hi, prot)| {
            let mut parts = Vec::new();
            if hi <= start || lo >= end { parts.push((lo, hi, prot)); }
            else {
                if lo < start { parts.push((lo, start, prot)); }
                if hi > end { parts.push((end, hi, prot)); }
            }
            parts
        }).collect();
        self.promoted_exec.retain(|&page| page < start || page >= end);
    }
    /// `Some(executable)` when `addr` falls inside a mapped region.
    fn region_for(&self, addr: u64) -> Option<bool> {
        let at = self.regions.partition_point(|r| r.0 <= addr);
        self.regions[..at]
            .iter()
            .rev()
            .find(|r| addr < r.1)
            .map(|r| r.2)
    }

    fn is_promoted_exec(&self, page: u64) -> bool {
        !self.promoted_exec.is_empty() && self.promoted_exec.binary_search(&page).is_ok()
    }

    fn promote_exec(&mut self, page: u64) {
        if let Err(at) = self.promoted_exec.binary_search(&page) {
            self.promoted_exec.insert(at, page);
        }
    }
}

pub struct UnicornCpu {
    uc: Unicorn<'static, ()>,
    last_hook: Rc<RefCell<Option<u32>>>,
    /// Address of the last invalid memory access, recorded so crash
    /// reports can name the faulting address instead of just the
    /// access kind.
    last_fault: Rc<RefCell<Option<(String, u64)>>>,
    stop_requested: Rc<RefCell<bool>>,
    /// Mapping table backing the virtual-TLB hook; `None` when the
    /// architectural TLB is in use and Unicorn resolves pages itself.
    guest_map: Option<Rc<RefCell<GuestMap>>>,
    arch: Arch,
    mips_status: u32,
    code_hooks: Vec<(u32, u32, UcHookId)>,
    images: Rc<RefCell<crate::image_pages::ImagePages>>,
}

impl UnicornCpu {
    pub fn new() -> Result<Self, CpuError> {
        Self::new_for_arch(Arch::Arm)
    }

    pub fn new_for_arch(arch: Arch) -> Result<Self, CpuError> {
        let (uc_arch, mode) = match arch {
            Arch::Arm => (UcArch::ARM, Mode::LITTLE_ENDIAN),
            Arch::Mips => (UcArch::MIPS, Mode::MIPS32 | Mode::LITTLE_ENDIAN),
        };
        let mut uc = Unicorn::new(uc_arch, mode)
            .map_err(|e| CpuError::Backend(format!("Unicorn::new failed: {e:?}")))?;
        if arch == Arch::Arm {
            let _ = uc.reg_write(RegisterARM::FPEXC, 0x4000_0000);
            let _ = uc.reg_write(RegisterARM::C1_C0_2, 0x00F0_0000);
        }
        let last_fault: Rc<RefCell<Option<(String, u64)>>> = Rc::new(RefCell::new(None));
        let images = Rc::new(RefCell::new(crate::image_pages::ImagePages::default()));
        let guest_map = install_virtual_tlb(&mut uc, &last_fault, &images);
        if guest_map.is_none() {
            // Architectural TLB: Unicorn resolves pages itself, so the
            // only way to learn the faulting address is the
            // invalid-access hook.
            let sink = last_fault.clone();
            let pending = images.clone();
            let _ = uc.add_mem_hook(
                ::unicorn_engine::unicorn_const::HookType::MEM_INVALID,
                0,
                u64::MAX,
                move |uc, kind, addr, size, _value| {
                    if matches!(kind, MemType::READ_UNMAPPED | MemType::WRITE_UNMAPPED | MemType::FETCH_UNMAPPED) {
                        match materialize_images(uc, &pending, addr as u32, size as u32) {
                            Ok(true) => return true,
                            Err(error) => { *sink.borrow_mut() = Some((error.to_string(), addr)); return false; }
                            Ok(false) => {}
                        }
                    }
                    *sink.borrow_mut() = Some((format!("{kind:?} size={size}"), addr));
                    false
                },
            );
        }
        Ok(Self {
            uc,
            arch,
            last_fault,
            guest_map,
            last_hook: Rc::new(RefCell::new(None)),
            stop_requested: Rc::new(RefCell::new(false)),
            mips_status: 0,
            code_hooks: Vec::new(),
            images,
        })
    }
}

/// Serve the softmmu from our own mapping table instead of the guest's
/// page tables, and return that table.
///
/// This is the single biggest win available to a software-rendered
/// Pocket PC game. PocketHLE loads WinCE images flat and never builds
/// ARM page tables, so guest code runs with the MMU off — and QEMU's
/// MMU-disabled path hands back `PAGE_READ | PAGE_WRITE | PAGE_EXEC`
/// for *every* page. A page QEMU believes is executable keeps its
/// `TLB_NOTDIRTY` bit forever, because `notdirty_write()` only clears
/// it when the TLB entry has no code address; and a store to a
/// `TLB_NOTDIRTY` page abandons the TCG inline fast path for a C
/// helper. `jit_store_tlb_variants` in `tests/jit_microbench.rs`
/// measures the difference: 38.1 ns per guest store with the
/// architectural TLB, 3.6 ns with this one.
///
/// Zuma paints its 800x480 frame with plain `STR` instructions rather
/// than a memcpy, so that 10x store penalty *was* its frame budget.
/// Marking data pages non-executable is what lets QEMU clear
/// `TLB_NOTDIRTY` and inline them.
///
/// The same measurement shows the fast path also requires that no
/// memory hook covers the address (`uc_mem_hook_installed()` is the
/// third condition in `notdirty_write()`), which is why the caller only
/// installs the `MEM_INVALID` hook when this returns `None`: fault
/// addresses are recorded here instead, and with the access type.
///
/// Set `POCKETHLE_CPU_TLB=1` to keep Unicorn's architectural TLB, which
/// restores the pre-optimisation behaviour exactly.
fn install_virtual_tlb(
    uc: &mut Unicorn<'static, ()>,
    last_fault: &Rc<RefCell<Option<(String, u64)>>>,
    images: &Rc<RefCell<crate::image_pages::ImagePages>>,
) -> Option<Rc<RefCell<GuestMap>>> {
    if std::env::var_os("POCKETHLE_CPU_TLB").is_some() {
        return None;
    }
    if uc.ctl_set_tlb_type(TlbType::VIRTUAL).is_err() {
        return None;
    }
    let map: Rc<RefCell<GuestMap>> = Rc::new(RefCell::new(GuestMap::default()));
    let regions = map.clone();
    let sink = last_fault.clone();
    // `begin > end` is Unicorn's "every address" bound check. The
    // address handed to the callback is already page-aligned.
    let pending = images.clone();
    let installed = uc.add_tlb_hook(1, 0, move |uc, page, kind| {
        if let Err(error) = materialize_images(uc, &pending, page as u32, 1) {
            *sink.borrow_mut() = Some((error.to_string(), page));
            return None;
        }
        let mut map = regions.borrow_mut();
        let Some(region_exec) = map.region_for(page) else {
            *sink.borrow_mut() = Some((format!("{kind:?} unmapped"), page));
            return None;
        };
        if let Some(&(_, _, prot)) = map.protected.iter().rev().find(|&&(start, end, _)| page >= start && page < end) {
            let required = match kind { MemType::FETCH => Prot::EXEC, MemType::WRITE => Prot::WRITE, _ => Prot::READ };
            if !prot.contains(required) {
                *sink.borrow_mut() = Some((format!("{kind:?} protected"), page));
                return None;
            }
            return Some(TlbEntry { paddr: page, perms: map_prot(prot) });
        }
        // Report read/write for anything mapped, matching what the
        // MMU-disabled ARM walk used to grant: Unicorn still enforces
        // the region's own `UC_PROT_*` bits in its store/load helper,
        // and a page we hand back as non-executable stays on the slow
        // helper path anyway whenever it is genuinely read-only.
        let mut perms = UcProt::READ | UcProt::WRITE;
        if region_exec || map.is_promoted_exec(page) {
            perms |= UcProt::EXEC;
        } else if kind == MemType::FETCH {
            // A guest running code out of a page it mapped as data —
            // a runtime-built trampoline — has to keep working.
            // Remembering the promotion is also what keeps QEMU
            // invalidating that page's translations on later writes:
            // an entry with no code address would let stores go
            // inline and leave stale translated code behind.
            map.promote_exec(page);
            perms |= UcProt::EXEC;
        }
        Some(TlbEntry { paddr: page, perms })
    });
    if installed.is_err() {
        // Without the hook the virtual TLB would derive permissions
        // from the access type alone, so a page used for both loads
        // and stores would refill on every access. Go back to the
        // architectural TLB instead.
        let _ = uc.ctl_set_tlb_type(TlbType::CPU);
        return None;
    }
    Some(map)
}

/// Called only by a TLB miss, an invalid access, or an HLE host access.
/// No per-instruction/read/write hook is installed on the normal fast path.
fn materialize_images(uc: &mut Unicorn<'_, ()>, images: &Rc<RefCell<crate::image_pages::ImagePages>>,
    va: u32, len: u32) -> Result<bool, CpuError> {
    let mut found = false;
    for address in crate::image_pages::ImagePages::range(va, len)? {
        // Unicorn's host mem_write can itself refill the virtual TLB. Never
        // keep a RefCell borrow across a backend call that may reenter a hook.
        let image = {
            let mut pending = images.borrow_mut();
            if pending.pages.get(&address).is_some_and(|image| image.resident) {
                found = true;
                continue;
            }
            pending.pages.remove(&address)
        };
        let Some(mut image) = image else { continue; };
        found = true;
        if !image.budget.acquire() {
            images.borrow_mut().pages.insert(address, image);
            return Err(CpuError::ImageOutOfMemory { va: address });
        }
        let result = uc.mem_map(address as u64, 4096, map_prot(image.prot));
        if let Err(error) = result {
            image.budget.release();
            images.borrow_mut().pages.insert(address, image);
            return Err(CpuError::Backend(format!("image page-in mem_map: {error:?}")));
        }
        if !image.bytes.is_empty() {
            if let Err(error) = uc.mem_write(address as u64, &image.bytes) {
                let _ = uc.mem_unmap(address as u64, 4096);
                image.budget.release();
                images.borrow_mut().pages.insert(address, image);
                return Err(CpuError::Backend(format!("image page-in mem_write: {error:?}")));
            }
        }
        image.resident = true;
        image.bytes = Vec::new();
        images.borrow_mut().pages.insert(address, image);
    }
    Ok(found)
}

fn map_prot(p: Prot) -> UcProt {
    let mut m = UcProt::NONE;
    if p.contains(Prot::READ) {
        m |= UcProt::READ;
    }
    if p.contains(Prot::WRITE) {
        m |= UcProt::WRITE;
    }
    if p.contains(Prot::EXEC) {
        m |= UcProt::EXEC;
    }
    m
}

fn map_arm_reg(r: ArmReg) -> RegisterARM {
    use ArmReg::*;
    match r {
        R0 => RegisterARM::R0,
        R1 => RegisterARM::R1,
        R2 => RegisterARM::R2,
        R3 => RegisterARM::R3,
        R4 => RegisterARM::R4,
        R5 => RegisterARM::R5,
        R6 => RegisterARM::R6,
        R7 => RegisterARM::R7,
        R8 => RegisterARM::R8,
        R9 => RegisterARM::R9,
        R10 => RegisterARM::R10,
        R11 => RegisterARM::R11,
        R12 => RegisterARM::R12,
        Sp => RegisterARM::SP,
        Lr => RegisterARM::LR,
        Pc => RegisterARM::PC,
        Cpsr => RegisterARM::CPSR,
    }
}

fn map_mips_reg(r: ArmReg) -> RegisterMIPS {
    use ArmReg::*;
    match r {
        R0 => RegisterMIPS::A0,
        R1 => RegisterMIPS::A1,
        R2 => RegisterMIPS::A2,
        R3 => RegisterMIPS::A3,
        R4 => RegisterMIPS::S0,
        R5 => RegisterMIPS::S1,
        R6 => RegisterMIPS::S2,
        R7 => RegisterMIPS::S3,
        R8 => RegisterMIPS::S4,
        R9 => RegisterMIPS::S5,
        R10 => RegisterMIPS::S6,
        R11 => RegisterMIPS::S7,
        R12 => RegisterMIPS::GP,
        Sp => RegisterMIPS::SP,
        Lr => RegisterMIPS::RA,
        Pc => RegisterMIPS::PC,
        Cpsr => RegisterMIPS::DSPCARRY,
    }
}

/// Optional per-slice wall-clock watchdog, in microseconds.
///
/// Returns `0` (disabled) by default. We deliberately do **not** bound
/// a slice by an instruction *count*: passing a non-zero `count` to
/// `uc_emu_start` makes Unicorn install an internal per-instruction
/// hook that disables QEMU's translation-block chaining, which costs
/// roughly 5-10x throughput on tight guest loops (this is exactly why
/// the JIT microbenchmark — which calls `emu_start(.., 0, 0)` — runs
/// far faster than real games used to). The thunk code hooks already
/// stop emulation on every WinCE API call, so the host frame hook and
/// stop requests still get a turn on any normal game frame. The
/// watchdog is only a safety net for a pathological guest that loops
/// forever without ever calling an API; set
/// `POCKETHLE_SLICE_TIMEOUT_MS` to enable it.
fn slice_watchdog_us() -> u64 {
    static CACHED: OnceLock<u64> = OnceLock::new();
    *CACHED.get_or_init(|| {
        std::env::var("POCKETHLE_SLICE_TIMEOUT_MS")
            .ok()
            .and_then(|v| v.trim().parse::<u64>().ok())
            .map(|ms| ms.saturating_mul(1000))
            .unwrap_or(0)
    })
}

impl Cpu for UnicornCpu {
    fn arch(&self) -> Arch {
        self.arch
    }

    fn supports_image_paging(&self) -> bool { self.arch == Arch::Arm }

    fn map_image_region(&mut self, va: u32, size: u32, prot: Prot, bytes: Vec<u8>,
        budget: std::sync::Arc<dyn crate::image_pages::ImagePageBudget>) -> Result<bool, CpuError> {
        if self.arch != Arch::Arm {
            self.map_region(va, size, prot)?;
            self.write_mem(va, &bytes)?;
            return Ok(false);
        }
        let end = u64::from(va) + u64::from(size);
        let regions = self.uc.mem_regions().map_err(|e| CpuError::Backend(format!("mem_regions: {e:?}")))?;
        if regions.iter().any(|region| region.begin < end && region.end >= u64::from(va)) {
            return Err(CpuError::BadMemory { va, size });
        }
        self.images.borrow_mut().reserve(va, size, prot, bytes, budget)?;
        if let Some(map) = &self.guest_map {
            map.borrow_mut().insert(va as u64, size as u64, prot.contains(Prot::EXEC));
        }
        let _ = self.uc.ctl_flush_tlb();
        Ok(true)
    }

    fn map_region(&mut self, va: u32, size: u32, prot: Prot) -> Result<(), CpuError> {
        if self.images.borrow().pages.range(va..va.saturating_add(size)).next().is_some() {
            return Err(CpuError::BadMemory { va, size });
        }
        self.uc
            .mem_map(va as u64, size as u64, map_prot(prot))
            .map_err(|e| CpuError::Backend(format!("mem_map: {e:?}")))?;
        if let Some(map) = &self.guest_map {
            map.borrow_mut()
                .insert(va as u64, size as u64, prot.contains(Prot::EXEC));
            // Pages inside the new region may already have a
            // "fault" verdict cached from a probe, so drop the TLB.
            // Mappings are created at load time and by VirtualAlloc,
            // never on a hot path.
            let _ = self.uc.ctl_flush_tlb();
        }
        Ok(())
    }

    fn unmap_region(&mut self, va: u32, size: u32) -> Result<(), CpuError> {
        let end_va = va.checked_add(size).ok_or(CpuError::BadMemory { va, size })?;
        let lazy = self.images.borrow().pages.range(va..end_va).next().is_some();
        if lazy {
            if va % 4096 != 0 || size % 4096 != 0 || (va..end_va).step_by(4096)
                .any(|page| !self.images.borrow().pages.contains_key(&page)) {
                return Err(CpuError::BadMemory { va, size });
            }
            for page in (va..end_va).step_by(4096) {
                let image = self.images.borrow_mut().pages.remove(&page).unwrap();
                if image.resident {
                    if let Err(error) = self.uc.mem_unmap(page as u64, 4096) {
                        self.images.borrow_mut().pages.insert(page, image);
                        return Err(CpuError::Backend(format!("mem_unmap image: {error:?}")));
                    }
                }
                // Drop refunds a committed page; cold backing has no RAM charge.
            }
        } else {
            self.uc.mem_unmap(va as u64, size as u64)
                .map_err(|e| CpuError::Backend(format!("mem_unmap: {e:?}")))?;
        }
        if let Some(map) = &self.guest_map { map.borrow_mut().remove(va as u64, size as u64); }
        let end = u64::from(va) + u64::from(size);
        let mut index = 0;
        while index < self.code_hooks.len() {
            let (lo, hi, id) = self.code_hooks[index];
            if lo >= va && u64::from(hi) < end {
                self.uc.remove_hook(id).map_err(|e| CpuError::Backend(format!("remove_hook: {e:?}")))?;
                self.code_hooks.remove(index);
            } else { index += 1; }
        }
        let _ = self.uc.ctl_remove_cache(u64::from(va), end - 1);
        let _ = self.uc.ctl_flush_tlb();
        Ok(())
    }
    fn protect_region(&mut self, va: u32, size: u32, prot: Prot) -> Result<(), CpuError> {
        materialize_images(&mut self.uc, &self.images, va, size)?;
        self.uc.mem_protect(va as u64, size as u64, map_prot(prot))
            .map_err(|error| CpuError::Backend(format!("mem_protect: {error:?}")))?;
        if let Some(map) = &self.guest_map {
            map.borrow_mut().protected.push((va as u64, va as u64 + size as u64, prot));
        }
        let _ = self.uc.ctl_flush_tlb();
        Ok(())
    }

    fn check_guest_access(&self,va:u32,len:u32,required:Prot)->Result<(),CpuError>{
        let regions=self.uc.mem_regions().map_err(|e|CpuError::Backend(format!("mem_regions: {e:?}")))?;
        // Bindgen's C enum underlying type differs between Windows and Unix.
        let images=self.images.borrow();let permissions=map_prot(required).0 as u32;
        for page in crate::image_pages::ImagePages::range(va,len)? {
            let allowed=if let Some(r)=regions.iter().find(|r|r.begin<=page as u64&&r.end>=page as u64){
                (r.perms as u32) & permissions == permissions
            }else{images.pages.get(&page).is_some_and(|p|!p.resident&&p.prot.contains(required))};
            if !allowed{return Err(CpuError::BadMemory{va,size:len});}
        }
        Ok(())
    }
    fn write_mem(&mut self, va: u32, data: &[u8]) -> Result<(), CpuError> {
        materialize_images(&mut self.uc, &self.images, va, data.len() as u32)?;
        self.uc
            .mem_write(va as u64, data)
            .map_err(|e| CpuError::Backend(format!("mem_write: {e:?}")))
    }

    fn read_mem(&mut self, va: u32, len: u32) -> Result<Vec<u8>, CpuError> {
        materialize_images(&mut self.uc, &self.images, va, len)?;
        let mut out = vec![0u8; len as usize];
        self.uc
            .mem_read(va as u64, &mut out)
            .map_err(|e| CpuError::Backend(format!("mem_read: {e:?}")))?;
        Ok(out)
    }

    fn read_mem_into(&mut self, va: u32, dst: &mut [u8]) -> Result<(), CpuError> {
        materialize_images(&mut self.uc, &self.images, va, dst.len() as u32)?;
        // Bypass the default `read_mem` -> Vec allocation: feed
        // unicorn's `mem_read` the caller's buffer directly. Used by
        // the per-frame GAPI flush (~150 KiB).
        self.uc
            .mem_read(va as u64, dst)
            .map_err(|e| CpuError::Backend(format!("mem_read: {e:?}")))
    }

    fn read_reg(&mut self, reg: ArmReg) -> Result<u32, CpuError> {
        if self.arch == Arch::Mips && reg == ArmReg::Cpsr {
            return Ok(self.mips_status);
        }
        let value = match self.arch {
            Arch::Arm => self.uc.reg_read(map_arm_reg(reg)),
            Arch::Mips => self.uc.reg_read(map_mips_reg(reg)),
        };
        value
            .map(|v| v as u32)
            .map_err(|e| CpuError::Backend(format!("reg_read: {e:?}")))
    }

    fn read_fpscr(&mut self) -> Result<u32, CpuError> {
        if self.arch != Arch::Arm { return Ok(0); }
        self.uc.reg_read(RegisterARM::FPSCR).map(|v| v as u32)
            .map_err(|e| CpuError::Backend(format!("FPSCR read: {e:?}")))
    }
    fn write_fpscr(&mut self, value: u32) -> Result<(), CpuError> {
        if self.arch != Arch::Arm { return Ok(()); }
        self.uc.reg_write(RegisterARM::FPSCR, u64::from(value))
            .map_err(|e| CpuError::Backend(format!("FPSCR write: {e:?}")))
    }

    fn write_reg(&mut self, reg: ArmReg, value: u32) -> Result<(), CpuError> {
        if self.arch == Arch::Mips && reg == ArmReg::Cpsr {
            self.mips_status = value;
            return Ok(());
        }
        let result = match self.arch {
            Arch::Arm => self.uc.reg_write(map_arm_reg(reg), value as u64),
            Arch::Mips => self.uc.reg_write(map_mips_reg(reg), value as u64),
        };
        result.map_err(|e| CpuError::Backend(format!("reg_write: {e:?}")))
    }

    fn read_return(&mut self) -> Result<u32, CpuError> {
        if self.arch == Arch::Mips {
            return self
                .uc
                .reg_read(RegisterMIPS::V0)
                .map(|v| v as u32)
                .map_err(|e| CpuError::Backend(format!("reg_read: {e:?}")));
        }
        self.read_reg(ArmReg::R0)
    }

    fn write_return(&mut self, value: u32) -> Result<(), CpuError> {
        if self.arch == Arch::Mips {
            return self
                .uc
                .reg_write(RegisterMIPS::V0, value as u64)
                .map_err(|e| CpuError::Backend(format!("reg_write: {e:?}")));
        }
        self.write_reg(ArmReg::R0, value)
    }

    fn write_return_pair(&mut self, first: u32, second: u32) -> Result<(), CpuError> {
        if self.arch == Arch::Mips {
            self.uc
                .reg_write(RegisterMIPS::V0, first as u64)
                .map_err(|e| CpuError::Backend(format!("reg_write: {e:?}")))?;
            return self
                .uc
                .reg_write(RegisterMIPS::V1, second as u64)
                .map_err(|e| CpuError::Backend(format!("reg_write: {e:?}")));
        }
        self.write_return(first)?;
        self.write_reg(ArmReg::R1, second)
    }

    fn add_code_hook(&mut self, va: u32) -> Result<(), CpuError> {
        self.add_code_hook_range(va, va)
    }

    fn add_instruction_hook(&mut self, va: u32) -> Result<(), CpuError> {
        self.add_code_hook(va)
    }

    fn add_code_hook_range(&mut self, lo: u32, hi: u32) -> Result<(), CpuError> {
        let last = self.last_hook.clone();
        let stop = self.stop_requested.clone();
        // Report the address that actually trapped, not the range's
        // bounds: with one hook covering a whole run of thunk slots
        // the run loop identifies the import by the trapping PC.
        let cb = move |uc: &mut Unicorn<'_, ()>, addr: u64, _size: u32| {
            *last.borrow_mut() = Some(addr as u32);
            *stop.borrow_mut() = true;
            let _ = uc.emu_stop();
        };
        let id = self.uc.add_code_hook(lo as u64, hi as u64, cb)
            .map_err(|e| CpuError::Backend(format!("add_code_hook: {e:?}")))?;
        self.code_hooks.push((lo, hi, id));
        Ok(())
    }

    fn run_until_hook(
        &mut self,
        start_va: u32,
        _max_instructions: u64,
    ) -> Result<StopReason, CpuError> {
        *self.last_hook.borrow_mut() = None;
        *self.stop_requested.borrow_mut() = false;
        // IMPORTANT: run with `count = 0` (no instruction limit) so the
        // QEMU TCG keeps chaining translation blocks at full speed. A
        // non-zero `count` would silently install a per-instruction
        // counting hook and tank throughput ~5-10x. Slices are instead
        // ended by the IAT-thunk code hooks (which call `emu_stop` on
        // every emulated API call) and, optionally, by a wall-clock
        // watchdog for pathological API-free loops.
        let r = self.uc.emu_start(
            start_va as u64,
            0,                   // until = 0 → run until stopped
            slice_watchdog_us(), // timeout (us); 0 = no timeout
            0,                   // count = 0 → keep TB chaining (do NOT pass a limit)
        );
        if let Some(addr) = *self.last_hook.borrow() {
            return Ok(StopReason::Hook(addr));
        }
        match r {
            // No hook fired: either an explicit stop was requested from
            // another thread/hook, or the watchdog timeout elapsed.
            // Both are benign slice boundaries — the caller refreshes
            // state and resumes from the current PC.
            Ok(()) => {
                if *self.stop_requested.borrow() {
                    Ok(StopReason::Requested)
                } else {
                    Ok(StopReason::InstructionLimit)
                }
            }
            Err(e) => {
                if let Some((kind, addr)) = self.last_fault.borrow().clone() {
                    Err(CpuError::Backend(format!(
                        "emu_start: {e:?} ({kind}) at guest address 0x{addr:08x}"
                    )))
                } else {
                    Err(CpuError::Backend(format!("emu_start: {e:?}")))
                }
            }
        }
    }

    fn request_stop(&mut self) {
        *self.stop_requested.borrow_mut() = true;
        let _ = self.uc.emu_stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unmap_and_remap_do_not_keep_old_hooks_or_translated_arm_code() {
        let mut cpu = UnicornCpu::new_for_arch(Arch::Arm).unwrap();
        cpu.map_region(0x1000, 0x1000, Prot::ALL).unwrap();
        cpu.map_region(0x3000, 0x1000, Prot::ALL).unwrap();
        cpu.write_mem(0x1000, &[0x01, 0x00, 0xa0, 0xe3, 0x3d, 0x00, 0x00, 0xea]).unwrap();
        cpu.write_mem(0x1100, &0xe12fff1eu32.to_le_bytes()).unwrap();
        cpu.add_code_hook(0x1100).unwrap();
        cpu.add_code_hook(0x3000).unwrap();
        cpu.write_reg(ArmReg::Lr, 0x3000).unwrap();
        assert_eq!(cpu.run_until_hook(0x1000, 100).unwrap(), StopReason::Hook(0x1100));
        assert_eq!(cpu.read_reg(ArmReg::R0).unwrap(), 1);
        cpu.unmap_region(0x1000, 0x1000).unwrap();
        assert!(cpu.read_mem(0x1000, 1).is_err());
        cpu.map_region(0x1000, 0x1000, Prot::ALL).unwrap();
        cpu.write_mem(0x1000, &[0x02, 0x00, 0xa0, 0xe3, 0x3d, 0x00, 0x00, 0xea]).unwrap();
        cpu.write_mem(0x1100, &0xe12fff1eu32.to_le_bytes()).unwrap();
        assert_eq!(cpu.run_until_hook(0x1000, 100).unwrap(), StopReason::Hook(0x3000));
        assert_eq!(cpu.read_reg(ArmReg::R0).unwrap(), 2);
    }

    #[test]
    fn unmapping_removes_permission_and_execution_promotion_history() {
        let mut map = GuestMap::default();
        map.insert(0x1000, 0x3000, false);
        map.protected.push((0x1000, 0x4000, Prot::READ));
        map.promote_exec(0x1000); map.promote_exec(0x2000); map.promote_exec(0x3000);
        map.remove(0x2000, 0x1000);
        assert_eq!(map.region_for(0x2000), None);
        assert!(!map.is_promoted_exec(0x2000));
        assert_eq!(map.protected, vec![(0x1000, 0x2000, Prot::READ), (0x3000, 0x4000, Prot::READ)]);
        map.insert(0x2000, 0x1000, true);
        assert_eq!(map.region_for(0x2000), Some(true));
    }
}
