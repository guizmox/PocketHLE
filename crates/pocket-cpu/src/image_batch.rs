//! First-touch image mappings grouped only within the requested range.
use crate::image_pages::{ImagePage, ImagePages};
use crate::{CpuError, Prot};
use std::{cell::RefCell, rc::Rc};

const PAGE: u32 = 4096;
// Bound staging memory and mapping size; this is not a prefetch window.
const MAX_PAGES: usize = 256;

pub(crate) trait ImageBackend {
    fn map(&mut self, address: u32, size: u32, prot: Prot) -> Result<(), CpuError>;
    fn write(&mut self, address: u32, bytes: &[u8]) -> Result<(), CpuError>;
    fn unmap(&mut self, address: u32, size: u32) -> Result<(), CpuError>;
}

fn restore(images: &Rc<RefCell<ImagePages>>, batch: Vec<(u32, ImagePage)>, resident: bool) {
    let mut images = images.borrow_mut();
    for (address, mut image) in batch {
        if resident {
            image.resident = true;
            image.bytes = Vec::new();
        } else {
            image.budget.release();
        }
        images.pages.insert(address, image);
    }
}

fn publish(
    backend: &mut impl ImageBackend,
    images: &Rc<RefCell<ImagePages>>,
    batch: Vec<(u32, ImagePage)>,
) -> Result<(), CpuError> {
    let start = batch[0].0;
    let size = batch.len() as u32 * PAGE;
    // A backend map starts zero-filled. BSS-only batches need no staging buffer
    // or host writes. Preserve initialized bytes and zero padding in mixed runs.
    let initialized = batch
        .iter()
        .enumerate()
        .filter(|(_, p)| !p.1.bytes.is_empty())
        .map(|(i, p)| i * PAGE as usize + p.1.bytes.len())
        .max()
        .unwrap_or(0);
    let mut bytes = vec![0; initialized];
    for (i, (_, image)) in batch.iter().enumerate() {
        if !image.bytes.is_empty() {
            let offset = i * PAGE as usize;
            bytes[offset..offset + image.bytes.len()].copy_from_slice(&image.bytes);
        }
    }
    if let Err(error) = backend.map(start, size, batch[0].1.prot) {
        restore(images, batch, false);
        return Err(error);
    }
    if !bytes.is_empty() {
        if let Err(error) = backend.write(start, &bytes) {
            if let Err(rollback) = backend.unmap(start, size) {
                // Keep ownership/RAM accounting of memory the backend retained.
                // Execution fails; do not silently resume with partial contents.
                restore(images, batch, true);
                return Err(CpuError::Backend(format!(
                    "image batch initialization failed: {error}; rollback failed: {rollback}"
                )));
            }
            restore(images, batch, false);
            return Err(error);
        }
    }
    restore(images, batch, true);
    Ok(())
}

pub(crate) fn materialize(
    backend: &mut impl ImageBackend,
    images: &Rc<RefCell<ImagePages>>,
    va: u32,
    len: u32,
) -> Result<bool, CpuError> {
    let mut range = ImagePages::range(va, len)?.peekable();
    let mut found = false;
    while let Some(address) = range.next() {
        let image = {
            let mut images = images.borrow_mut();
            if images.pages.get(&address).is_some_and(|p| p.resident) {
                found = true;
                continue;
            }
            images.pages.remove(&address)
        };
        let Some(image) = image else { continue };
        found = true;
        if !image.budget.acquire() {
            images.borrow_mut().pages.insert(address, image);
            return Err(CpuError::ImageOutOfMemory { va: address });
        }
        let prot = image.prot;
        let mut batch = vec![(address, image)];
        let mut out_of_ram = None;
        while batch.len() < MAX_PAGES {
            let Some(&next) = range.peek() else { break };
            // No resident pages, holes, protection changes, or unrequested pages
            // may enter this mapping. Every page acquires its own device budget.
            if !images
                .borrow()
                .pages
                .get(&next)
                .is_some_and(|p| !p.resident && p.prot == prot)
            {
                break;
            }
            let image = images.borrow_mut().pages.remove(&next).unwrap();
            if !image.budget.acquire() {
                images.borrow_mut().pages.insert(next, image);
                out_of_ram = Some(next);
                break;
            }
            range.next();
            batch.push((next, image));
        }
        // Never retain a RefCell borrow across a backend operation: Unicorn can
        // reenter its virtual-TLB hook while initializing mapped image memory.
        publish(backend, images, batch)?;
        if let Some(va) = out_of_ram {
            // Match the original path: the affordable prefix remains resident.
            return Err(CpuError::ImageOutOfMemory { va });
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image_pages::ImagePageBudget;
    use std::collections::BTreeMap;
    use std::sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    };

    struct Budget {
        used: AtomicU32,
        limit: AtomicU32,
    }
    impl Budget {
        fn new(limit: u32) -> Arc<Self> {
            Arc::new(Self {
                used: AtomicU32::new(0),
                limit: AtomicU32::new(limit),
            })
        }
        fn count(&self) -> u32 {
            self.used.load(Ordering::SeqCst)
        }
    }
    impl ImagePageBudget for Budget {
        fn acquire(&self) -> bool {
            self.used
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
                    (n < self.limit.load(Ordering::SeqCst)).then_some(n + 1)
                })
                .is_ok()
        }
        fn release(&self) {
            assert!(self.used.fetch_sub(1, Ordering::SeqCst) > 0);
        }
    }
    #[derive(Default)]
    struct Backend {
        pages: BTreeMap<u32, Vec<u8>>,
        maps: Vec<(u32, u32, Prot)>,
        writes: usize,
        fail_map: bool,
        fail_write: bool,
        fail_unmap: bool,
        reenter: Option<Rc<RefCell<ImagePages>>>,
    }
    impl ImageBackend for Backend {
        fn map(&mut self, address: u32, size: u32, prot: Prot) -> Result<(), CpuError> {
            if let Some(images) = &self.reenter {
                let _borrow = images.borrow_mut();
            }
            if self.fail_map {
                return Err(CpuError::Backend("map".into()));
            }
            for a in (address..address + size).step_by(PAGE as usize) {
                assert!(!self.pages.contains_key(&a));
                self.pages.insert(a, vec![0; PAGE as usize]);
            }
            self.maps.push((address, size, prot));
            Ok(())
        }
        fn write(&mut self, address: u32, bytes: &[u8]) -> Result<(), CpuError> {
            if let Some(images) = &self.reenter {
                let _borrow = images.borrow_mut();
            }
            self.writes += 1;
            if self.fail_write {
                return Err(CpuError::Backend("write".into()));
            }
            for (offset, &byte) in bytes.iter().enumerate() {
                let a = address + offset as u32;
                self.pages.get_mut(&(a & !(PAGE - 1))).unwrap()[(a % PAGE) as usize] = byte;
            }
            Ok(())
        }
        fn unmap(&mut self, address: u32, size: u32) -> Result<(), CpuError> {
            if self.fail_unmap {
                return Err(CpuError::Backend("unmap".into()));
            }
            for a in (address..address + size).step_by(PAGE as usize) {
                self.pages.remove(&a);
            }
            Ok(())
        }
    }
    fn reserve(
        images: &Rc<RefCell<ImagePages>>,
        at: u32,
        pages: u32,
        prot: Prot,
        bytes: Vec<u8>,
        budget: &Arc<Budget>,
    ) {
        images
            .borrow_mut()
            .reserve(at, pages * PAGE, prot, bytes, budget.clone())
            .unwrap();
    }
    #[test]
    fn large_bss_uses_one_map_without_prefetch_or_initialization_copy() {
        let images = Rc::new(RefCell::new(ImagePages::default()));
        let budget = Budget::new(1000);
        reserve(
            &images,
            PAGE,
            200,
            Prot::READ | Prot::WRITE,
            vec![],
            &budget,
        );
        let mut backend = Backend {
            reenter: Some(images.clone()),
            ..Default::default()
        };
        assert!(materialize(&mut backend, &images, PAGE, 728 * 1024).unwrap());
        assert_eq!(backend.maps.len(), 1);
        assert_eq!(backend.maps[0].1, 728 * 1024);
        assert_eq!(backend.writes, 0);
        assert_eq!(budget.count(), 182);
        assert!(!images.borrow().pages[&(183 * PAGE)].resident);
        assert!(materialize(&mut backend, &images, PAGE, 728 * 1024).unwrap());
        assert_eq!(backend.maps.len(), 1);
        drop(backend);
        drop(images);
        assert_eq!(budget.count(), 0);
    }
    #[test]
    fn initialized_bytes_padding_and_protection_boundaries_are_preserved() {
        let images = Rc::new(RefCell::new(ImagePages::default()));
        let a = Budget::new(8);
        let b = Budget::new(8);
        reserve(&images, PAGE, 2, Prot::READ, vec![7; 4101], &a);
        reserve(&images, PAGE * 3, 1, Prot::READ | Prot::WRITE, vec![9], &b);
        let mut backend = Backend {
            reenter: Some(images.clone()),
            ..Default::default()
        };
        materialize(&mut backend, &images, PAGE + 100, PAGE * 3 - 100).unwrap();
        assert_eq!(backend.maps.len(), 2);
        assert_eq!(backend.maps[0], (PAGE, PAGE * 2, Prot::READ));
        assert_eq!(backend.pages[&(2 * PAGE)][4], 7);
        assert_eq!(backend.pages[&(2 * PAGE)][5], 0);
        assert_eq!(backend.pages[&(3 * PAGE)][0], 9);
        assert_eq!(a.count(), 2);
        assert_eq!(b.count(), 1);
    }
    #[test]
    fn existing_pages_and_holes_are_never_remapped() {
        let images = Rc::new(RefCell::new(ImagePages::default()));
        let budget = Budget::new(8);
        reserve(&images, PAGE, 3, Prot::READ, vec![3], &budget);
        reserve(&images, PAGE * 5, 1, Prot::READ, vec![5], &budget);
        let mut backend = Backend::default();
        materialize(&mut backend, &images, PAGE * 2, 1).unwrap();
        backend.pages.get_mut(&(PAGE * 2)).unwrap()[0] = 99;
        materialize(&mut backend, &images, PAGE, PAGE * 5).unwrap();
        assert_eq!(backend.maps.len(), 4);
        assert_eq!(backend.pages[&(PAGE * 2)][0], 99);
        assert!(!backend.pages.contains_key(&(PAGE * 4)));
        assert_eq!(budget.count(), 4);
    }
    #[test]
    fn out_of_ram_keeps_only_the_affordable_prefix_and_can_retry() {
        let images = Rc::new(RefCell::new(ImagePages::default()));
        let budget = Budget::new(2);
        reserve(
            &images,
            PAGE,
            4,
            Prot::ALL,
            vec![4; 4 * PAGE as usize],
            &budget,
        );
        let mut backend = Backend::default();
        assert!(matches!(materialize(&mut backend, &images, PAGE, 4 * PAGE),
            Err(CpuError::ImageOutOfMemory { va }) if va == 3 * PAGE));
        assert_eq!(budget.count(), 2);
        assert_eq!(backend.maps.len(), 1);
        assert!(!images.borrow().pages[&(3 * PAGE)].resident);
        budget.limit.store(4, Ordering::SeqCst);
        materialize(&mut backend, &images, PAGE, 4 * PAGE).unwrap();
        assert_eq!(budget.count(), 4);
        assert_eq!(backend.pages[&(4 * PAGE)][0], 4);
    }
    #[test]
    fn backend_failures_refund_once_keep_backing_and_are_retryable() {
        for fail_map in [true, false] {
            let images = Rc::new(RefCell::new(ImagePages::default()));
            let budget = Budget::new(4);
            reserve(
                &images,
                PAGE,
                3,
                Prot::ALL,
                vec![6; 3 * PAGE as usize],
                &budget,
            );
            let mut backend = Backend {
                fail_map,
                fail_write: !fail_map,
                ..Default::default()
            };
            assert!(materialize(&mut backend, &images, PAGE, 3 * PAGE).is_err());
            assert_eq!(budget.count(), 0);
            assert!(backend.pages.is_empty());
            assert!(images
                .borrow()
                .pages
                .values()
                .all(|p| !p.resident && !p.bytes.is_empty()));
            backend.fail_map = false;
            backend.fail_write = false;
            materialize(&mut backend, &images, PAGE, 3 * PAGE).unwrap();
            assert_eq!(budget.count(), 3);
            assert_eq!(backend.pages[&(3 * PAGE)][0], 6);
            drop(images);
            assert_eq!(budget.count(), 0);
        }
    }
    #[test]
    fn failed_rollback_retains_accounting_for_the_live_backend_mapping() {
        let images = Rc::new(RefCell::new(ImagePages::default()));
        let budget = Budget::new(3);
        reserve(&images, PAGE, 2, Prot::ALL, vec![3], &budget);
        let mut backend = Backend {
            fail_write: true,
            fail_unmap: true,
            ..Default::default()
        };
        assert!(materialize(&mut backend, &images, PAGE, 2 * PAGE).is_err());
        assert_eq!(budget.count(), 2);
        assert_eq!(backend.pages.len(), 2);
        assert!(images.borrow().pages.values().all(|p| p.resident));
    }
    #[test]
    fn bounded_batches_and_empty_or_wrapping_ranges() {
        let images = Rc::new(RefCell::new(ImagePages::default()));
        let budget = Budget::new(600);
        reserve(&images, PAGE, 600, Prot::READ, vec![], &budget);
        let mut backend = Backend::default();
        assert!(!materialize(&mut backend, &images, PAGE, 0).unwrap());
        assert!(materialize(&mut backend, &images, u32::MAX - 4, 16).is_err());
        materialize(&mut backend, &images, PAGE, 600 * PAGE).unwrap();
        assert_eq!(
            backend.maps.iter().map(|p| p.1 / PAGE).collect::<Vec<_>>(),
            vec![256, 256, 88]
        );
        assert_eq!(budget.count(), 600);
    }
    #[test]
    fn adjacent_pages_charge_separate_budgets_even_when_protections_match() {
        let images = Rc::new(RefCell::new(ImagePages::default()));
        let a = Budget::new(2);
        let b = Budget::new(0);
        reserve(&images, PAGE, 2, Prot::READ, vec![1], &a);
        reserve(&images, 3 * PAGE, 1, Prot::READ, vec![2], &b);
        let mut backend = Backend::default();
        assert!(matches!(materialize(&mut backend, &images, PAGE, 3 * PAGE),
            Err(CpuError::ImageOutOfMemory { va }) if va == 3 * PAGE));
        assert_eq!(a.count(), 2);
        assert_eq!(b.count(), 0);
        assert_eq!(backend.maps.len(), 1);
        b.limit.store(1, Ordering::SeqCst);
        materialize(&mut backend, &images, PAGE, 3 * PAGE).unwrap();
        assert_eq!(b.count(), 1);
        assert_eq!(backend.pages[&(3 * PAGE)][0], 2);
    }
}
