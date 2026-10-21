//! CE sections live outside the private 32 MiB process slot. File-backed
//! pages use the host's demand-paged file cache, rather than committed guest
//! heap RAM. Anonymous sections consume actual program RAM exactly once.
use crate::{
    memory_division::{pages, MemoryDivision},
    vfs::MappedFile,
};
use pocket_cpu::{
    shared_memory::{rounded_len, SharedMemory, SharedRegion},
    Cpu, Prot,
};
use std::{collections::BTreeMap, sync::Arc};

pub struct FileMapping {
    pub key: u32,
    pub size: u32,
    pub writable: bool,
    pub memory: Arc<SharedMemory>,
    _file: Option<MappedFile>,
    _ram: Option<ProgramCharge>,
}
struct ProgramCharge {
    ram: MemoryDivision,
    pages: u32,
}
impl Drop for ProgramCharge {
    fn drop(&mut self) {
        self.ram.release_program(self.pages);
    }
}
impl FileMapping {
    pub fn file(key: u32, size: u32, writable: bool, file: MappedFile) -> Arc<Self> {
        Arc::new(Self {
            key,
            size,
            writable,
            memory: file.memory.clone(),
            _file: Some(file),
            _ram: None,
        })
    }
    pub fn anonymous(key: u32, size: u32, ram: Option<&MemoryDivision>) -> Result<Arc<Self>, u32> {
        let charge = if let Some(ram) = ram {
            let count = pages(u64::from(size));
            if !ram.acquire_program(count) {
                return Err(8);
            }
            Some(ProgramCharge {
                ram: ram.clone(),
                pages: count,
            })
        } else {
            None
        };
        let memory = SharedMemory::anonymous(size).map_err(|_| 8u32)?;
        Ok(Arc::new(Self {
            key,
            size,
            writable: true,
            memory,
            _file: None,
            _ram: charge,
        }))
    }
}
pub struct View {
    pub object: Arc<FileMapping>,
    pub offset: u32,
    pub len: u32,
    pub mapped_len: u32,
    pub prot: Prot,
}
#[derive(Default)]
pub struct MappedViews {
    views: BTreeMap<u32, View>,
}
// Reserve a non-overlapping arena: below it are thread stacks/private heaps;
// above it begin API thunks at 0x70000000. Runtime DLLs stop at 0x50000000.
pub const VIEW_BASE: u32 = 0x60000000;
pub const VIEW_END: u32 = 0x70000000;
impl MappedViews {
    pub fn is_empty(&self) -> bool {
        self.views.is_empty()
    }
    pub fn containing(&self, address: u32) -> Option<(u32, &View)> {
        let (&base, view) = self.views.range(..=address).next_back()?;
        (address - base < view.mapped_len).then_some((base, view))
    }
    /// # Safety
    /// The device execution gate must serialize all CPU execution and host
    /// accesses to these shared pages until every alias has been unmapped.
    pub unsafe fn map(
        &mut self,
        cpu: &mut dyn Cpu,
        object: Arc<FileMapping>,
        access: u32,
        offset: u32,
        len: u32,
    ) -> Result<u32, u32> {
        let write = match access {
            2 | 6 | 0x000f001f => true,
            4 => false,
            _ => return Err(87),
        };
        if write && !object.writable {
            return Err(5);
        }
        if offset % 4096 != 0 || offset >= object.size {
            return Err(87);
        }
        let len = if len == 0 { object.size - offset } else { len };
        if u64::from(offset) + u64::from(len) > u64::from(object.size) {
            return Err(87);
        }
        let mapped_len = rounded_len(len).ok_or(87u32)?;
        let mut base = VIEW_BASE;
        for (&next, view) in &self.views {
            if base.checked_add(mapped_len).is_some_and(|end| end <= next) {
                break;
            }
            base = (next + view.mapped_len).checked_add(65535).ok_or(8u32)? & !65535;
        }
        if base
            .checked_add(mapped_len)
            .is_none_or(|end| end > VIEW_END)
        {
            return Err(8);
        }
        let prot = if write {
            Prot::READ | Prot::WRITE
        } else {
            Prot::READ
        };
        let region = SharedRegion::new(object.memory.clone(), object.clone());
        // SAFETY: the shared device gate encloses guest execution and API I/O.
        // The retained FileMapping owns a VFS pin preventing file truncation.
        unsafe {
            cpu.map_shared_region(base, mapped_len, prot, region, offset)
                .map_err(|_| 8u32)?;
        }
        self.views.insert(
            base,
            View {
                object,
                offset,
                len,
                mapped_len,
                prot,
            },
        );
        Ok(base)
    }
    pub fn unmap(&mut self, cpu: &mut dyn Cpu, base: u32) -> Result<(), u32> {
        let view = self.views.get(&base).ok_or(87u32)?;
        cpu.unmap_region(base, view.mapped_len).map_err(|_| 87u32)?;
        self.views.remove(&base);
        Ok(())
    }
    pub fn flush(&self, address: u32, len: u32) -> Result<(), u32> {
        let (base, view) = self.containing(address).ok_or(87u32)?;
        let delta = address - base;
        if delta >= view.len {
            return Err(87);
        }
        let len = if len == 0 { view.len - delta } else { len };
        if len > view.len - delta {
            return Err(87);
        }
        view.object
            .memory
            .flush(view.offset + delta, len)
            .map_err(|_| 29)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        handles::{HandleObject, HandleTable},
        vfs::{SeekKind, Vfs},
    };
    use pocket_cpu::stub::StubCpu;

    #[test]
    fn section_aliases_share_bytes_across_processes_and_outlive_every_handle() {
        let dir = tempfile::tempdir().unwrap();
        let mut vfs = Vfs::new();
        vfs.mount_save_dir("\\SD Card\\", dir.path());
        let h = vfs
            .open_file("\\SD Card\\shared", 3, 0, 2, false, 0)
            .unwrap()
            .handle;
        assert_eq!(vfs.write(h, b"original"), Some(8));
        vfs.seek(h, 3, SeekKind::Begin).unwrap();
        let backing = vfs.mapping_backing(h, 8193, true).unwrap();
        assert_eq!(vfs.seek(h, 0, SeekKind::Current), Some(3));
        let mut handles = HandleTable::default();
        let (_, mut child) = handles.new_child().unwrap();
        let map = handles.reserve().unwrap();
        let object = FileMapping::file(map, 8193, true, backing);
        handles.insert_mapping(map, Some("same-name".into()), object.clone());
        let mut cpu = StubCpu::new();
        let mut cpu2 = StubCpu::new();
        let mut views = MappedViews::default();
        let mut views2 = MappedViews::default();
        let a = unsafe { views.map(&mut cpu, object.clone(), 2, 0, 0) }.unwrap();
        let b = unsafe { views.map(&mut cpu, object.clone(), 4, 4096, 4097) }.unwrap();
        assert_eq!(
            unsafe { views.map(&mut cpu, object.clone(), 2, 1, 1) },
            Err(87)
        );
        assert_eq!(
            unsafe { views.map(&mut cpu, object.clone(), 1, 0, 1) },
            Err(87)
        );
        assert_eq!(
            unsafe { views.map(&mut cpu, object.clone(), 4, 8192, 2) },
            Err(87)
        );
        assert!(cpu.check_guest_access(b, 1, Prot::WRITE).is_err());
        let transferred = child.duplicate(HandleObject::Mapping(map)).unwrap();
        let c = unsafe { views2.map(&mut cpu2, child.mapping(transferred).unwrap(), 2, 4096, 0) }
            .unwrap();
        vfs.close(h);
        handles.close(map);
        child.close(transferred);
        drop(object);
        cpu.write_mem(a + 4100, b"alive").unwrap();
        assert_eq!(cpu.read_mem(b + 4, 5).unwrap(), b"alive");
        assert_eq!(cpu2.read_mem(c + 4, 5).unwrap(), b"alive");
        cpu2.write_mem(c + 7, b"!").unwrap();
        assert_eq!(cpu.read_mem(a + 4100, 5).unwrap(), b"ali!e");
        assert!(handles.named_mapping("same-name").is_some());
        // The exclusive file lease is still owned by the live views.
        assert_eq!(
            vfs.open_file("\\SD Card\\shared", 3, 3, 3, false, 0),
            Err(32)
        );
        assert_eq!(views.unmap(&mut cpu, a + 1), Err(87));
        views.flush(a + 4100, 5).unwrap();
        assert_eq!(
            &std::fs::read(dir.path().join("shared")).unwrap()[4100..4105],
            b"ali!e"
        );
        assert_eq!(
            std::fs::metadata(dir.path().join("shared")).unwrap().len(),
            8193
        );
        views.unmap(&mut cpu, a).unwrap();
        views.unmap(&mut cpu, b).unwrap();
        views2.unmap(&mut cpu2, c).unwrap();
        assert!(handles.named_mapping("same-name").is_none());
        assert!(vfs
            .open_file("\\SD Card\\shared", 3, 0, 3, false, 0)
            .is_ok());
    }

    #[test]
    fn closing_handles_does_not_refund_anonymous_pages_while_cpu_owns_a_view() {
        let ram = MemoryDivision::new(32 * 4096, 16).unwrap();
        let mut handles = HandleTable::default();
        let h = handles.reserve().unwrap();
        let object = FileMapping::anonymous(h, 4097, Some(&ram)).unwrap();
        assert_eq!(ram.snapshot().program_used, 2);
        handles.insert_mapping(h, Some("anonymous".into()), object.clone());
        let mut cpu = StubCpu::new();
        let mut views = MappedViews::default();
        let a = unsafe { views.map(&mut cpu, object.clone(), 2, 0, 0) }.unwrap();
        let b = unsafe { views.map(&mut cpu, object.clone(), 4, 0, 0) }.unwrap();
        assert_eq!(ram.snapshot().program_used, 2);
        cpu.write_mem(a, b"shared").unwrap();
        assert_eq!(cpu.read_mem(b, 6).unwrap(), b"shared");
        handles.close(h);
        drop(object);
        drop(views); // CPU outlives the kernel view metadata during teardown.
        assert_eq!(ram.snapshot().program_used, 2);
        assert!(handles.named_mapping("anonymous").is_some());
        drop(cpu);
        assert_eq!(ram.snapshot().program_used, 0);
        assert!(handles.named_mapping("anonymous").is_none());
        assert!(matches!(
            FileMapping::anonymous(99, 17 * 4096, Some(&ram)),
            Err(8)
        ));
        assert_eq!(ram.snapshot().program_used, 0);
    }

    #[test]
    fn file_mapping_growth_respects_flash_quota_without_moving_the_cursor() {
        let dir = tempfile::tempdir().unwrap();
        let mut vfs = Vfs::new();
        vfs.mount_save_dir("\\Flash Disk\\", dir.path());
        let h = vfs
            .open_file("\\Flash Disk\\a", 3, 3, 2, false, 0)
            .unwrap()
            .handle;
        vfs.write(h, b"keep").unwrap();
        vfs.seek(h, 2, SeekKind::Begin);
        let before = vfs.disk_space("\\Flash Disk\\").unwrap();
        assert!(matches!(
            vfs.mapping_backing(h, 33 * 1024 * 1024, true),
            Err(112)
        ));
        assert_eq!(vfs.size(h), Some(4));
        assert_eq!(vfs.seek(h, 0, SeekKind::Current), Some(2));
        assert_eq!(vfs.disk_space("\\Flash Disk\\").unwrap(), before);
        let map = vfs.mapping_backing(h, 8193, true).unwrap();
        assert_eq!(
            vfs.disk_space("\\Flash Disk\\").unwrap().1,
            before.1 - (8193 - 4)
        );
        let second = vfs
            .open_file("\\Flash Disk\\a", 3, 3, 3, false, 0)
            .unwrap()
            .handle;
        vfs.seek(second, 0, SeekKind::Begin);
        assert!(!vfs.set_end_of_file(second));
        assert_eq!(vfs.write_failure_error(second), 1224);
        assert_eq!(
            vfs.open_file("\\Flash Disk\\a", 3, 3, 2, false, 0),
            Err(1224)
        );
        assert_eq!(vfs.size(second), Some(8193));
        drop(map);
        assert!(vfs.set_end_of_file(second));
        assert_eq!(vfs.size(h), Some(0));
    }

    #[test]
    fn ram_file_mapping_is_coherent_with_store_io_and_refunds_on_delete() {
        let ram = MemoryDivision::new(64 * 4096, 32).unwrap();
        let mut vfs = Vfs::new();
        vfs.attach_ram(Some(ram.clone()));
        let before = ram.snapshot().store_used;
        let h = vfs.open_file("\\mapped", 3, 3, 2, false, 0).unwrap().handle;
        vfs.write(h, b"before").unwrap();
        let backing = vfs.mapping_backing(h, 4097, true).unwrap();
        let object = FileMapping::file(99, 4097, true, backing);
        let mut views = MappedViews::default();
        let mut cpu = StubCpu::new();
        let a = unsafe { views.map(&mut cpu, object.clone(), 2, 0, 0) }.unwrap();
        cpu.write_mem(a + 1, b"MAP").unwrap();
        vfs.seek(h, 0, SeekKind::Begin);
        let mut bytes = [0; 6];
        vfs.read(h, &mut bytes).unwrap();
        assert_eq!(&bytes, b"bMAPre");
        vfs.seek(h, 2, SeekKind::Begin);
        vfs.write(h, b"IO").unwrap();
        assert_eq!(cpu.read_mem(a, 6).unwrap(), b"bMIOre");
        views.unmap(&mut cpu, a).unwrap();
        drop(object);
        vfs.seek(h, 6, SeekKind::Begin);
        assert!(vfs.set_end_of_file(h));
        vfs.seek(h, 0, SeekKind::Begin);
        vfs.read(h, &mut bytes).unwrap();
        assert_eq!(&bytes, b"bMIOre");
        vfs.close(h);
        vfs.delete("\\mapped").unwrap();
        assert_eq!(ram.snapshot().store_used, before);
        assert_eq!(ram.snapshot().program_used, 0);
    }

    #[test]
    fn read_only_sections_reject_write_views_and_preserve_exact_file_length() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a"), b"odd").unwrap();
        let mut vfs = Vfs::new();
        vfs.mount_read_only("\\ROM\\", dir.path());
        let h = vfs.open_file("\\ROM\\a", 1, 3, 3, false, 0).unwrap().handle;
        assert!(matches!(vfs.mapping_backing(h, 3, true), Err(5)));
        assert!(matches!(vfs.mapping_backing(h, 4, false), Err(5)));
        let object = FileMapping::file(99, 3, false, vfs.mapping_backing(h, 3, false).unwrap());
        let mut cpu = StubCpu::new();
        let mut views = MappedViews::default();
        assert_eq!(
            unsafe { views.map(&mut cpu, object.clone(), 2, 0, 0) },
            Err(5)
        );
        let a = unsafe { views.map(&mut cpu, object, 4, 0, 0) }.unwrap();
        assert_eq!(cpu.read_mem(a, 3).unwrap(), b"odd");
        assert!(cpu.write_mem(a, b"x").is_err());
        assert!(cpu
            .protect_region(a, 4096, Prot::READ | Prot::WRITE)
            .is_err());
        views.flush(a, 0).unwrap();
        views.unmap(&mut cpu, a).unwrap();
        assert_eq!(std::fs::read(dir.path().join("a")).unwrap(), b"odd");
    }
}
