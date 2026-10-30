use super::{Heap, HEAP_BASE};

#[test]
fn reservations_do_not_consume_ram_or_change_reported_capacity() {
    let mut heap=Heap::new(0x100000,0x40000);
    let before=heap.free_bytes();
    let a=heap.virtual_alloc(0,0x200000,true,false).unwrap();
    assert!(heap.external_virtual_block(a));
    assert_eq!(heap.free_bytes(),before);
    assert_eq!(heap.physical_free_bytes(),before);
    assert_eq!(heap.capacity_bytes(),before);
    assert!(heap.virtual_free(a,0,true));
    assert_eq!(heap.free_bytes(),before);
}
#[test]
fn contiguous_commit_uses_fragmented_backing_and_refunds_it() {
    let mut heap=Heap::new(0x100000,0x20000);
    heap.add_region(0x50000000,0x20000);
    let initial=heap.free_bytes();
    let a=heap.virtual_alloc(0,0x30000,true,true).unwrap();
    assert_eq!(heap.free_bytes(),initial-0x30000);
    assert_eq!(heap.program_pages(),48);
    assert_eq!(heap.virtual_alloc(a,0x30000,false,true),Some(a));
    assert_eq!(heap.program_pages(),48);
    assert!(heap.virtual_free(a+4096,4096,false));
    assert!(heap.virtual_free(a+4096,4096,false));
    assert_eq!(heap.program_pages(),47);
    assert_eq!(heap.virtual_alloc(a+4096,4096,false,true),Some(a+4096));
    assert_eq!(heap.program_pages(),48);
    assert!(heap.virtual_free(a,0,true));
    assert_eq!(heap.free_bytes(),initial);
    assert_eq!(heap.program_pages(),0);
    assert!(heap.alloc(0x1fff8).is_some());
    assert!(heap.alloc(0x1fff8).is_some());
}
#[test]
fn failed_commit_is_atomic_and_cannot_grow_physical_capacity() {
    let mut heap=Heap::new(0x100000,0x20000);
    let a=heap.virtual_alloc(0,0x40000,true,false).unwrap();
    assert!(heap.virtual_alloc(a,0x10000,false,true).is_some());
    let before=heap.free_bytes();let pages=heap.program_pages();
    assert_eq!(heap.virtual_alloc(a+0x10000,0x20000,false,true),None);
    assert_eq!(heap.free_bytes(),before);assert_eq!(heap.program_pages(),pages);
    assert_eq!(heap.virtual_block(a).unwrap().2.len(),16);
    assert!(heap.virtual_free(a,0,true));
    assert_eq!(heap.free_bytes(),0x20000);
    assert_eq!(heap.virtual_alloc(0,0x40000,true,true),None);
    assert_eq!(heap.free_bytes(),0x20000);
}
#[test]
fn reservations_reject_overlap_overflow_and_invalid_releases() {
    let mut heap=Heap::new(0x100000,0x40000);
    let a=heap.virtual_alloc(0,0x20000,true,false).unwrap();
    let b=heap.virtual_alloc(0,0x20000,true,false).unwrap();
    assert!(b>=a+0x20000);
    assert_eq!(heap.virtual_alloc(a+4096,0x10000,true,false),None);
    assert_eq!(heap.virtual_alloc(0,u32::MAX,true,false),None);
    assert_eq!(heap.virtual_alloc(a+0x1f000,0x2000,false,true),None);
    assert!(!heap.virtual_free(a+4096,0,true));
    assert!(!heap.virtual_free(a,4096,true));
    assert!(heap.virtual_free(a,0,true));
    assert_eq!(heap.virtual_alloc(0,0x10000,true,false),Some(a));
}
#[test]
fn large_pools_fit_without_increasing_ram() {
    let mut heap=Heap::new(0x760000,92930048-0x4000000);
    heap.add_region(HEAP_BASE,0x4000000);
    heap.alloc(274608).unwrap();
    assert_eq!(heap.free_bytes(),92655432);
    let initial=heap.free_bytes();
    let first=heap.virtual_alloc(0,54332672,true,false).unwrap();
    assert_eq!(heap.free_bytes(),initial);
    for offset in (0..54332672).step_by(65536) {
        heap.virtual_alloc(first+offset,65536.min(54332672-offset),false,true).unwrap();
    }
    let second=heap.virtual_alloc(0,27131904,true,false).unwrap();
    heap.virtual_alloc(second,27131904,false,true).unwrap();
    let third=heap.virtual_alloc(0,0x180000,true,true).unwrap();
    assert_eq!(heap.capacity_bytes(),92930048);
    for base in [second,third,first] { assert!(heap.virtual_free(base,0,true)); }
    assert_eq!(heap.free_bytes(),initial);
}
#[test]
fn query_reports_reservation_commit_protection_and_decommit_boundaries() {
    let mut heap=Heap::new(0x100000,0x40000);
    let a=heap.virtual_alloc(0,0x30000,true,false).unwrap();
    heap.set_virtual_allocation_protect(a,1);
    assert_eq!(heap.query_virtual(a+17),Some([a,a,1,0x30000,0x2000,0,0x20000]));
    heap.virtual_alloc(a+4096,8192,false,true).unwrap();
    heap.set_virtual_page_protect(a+4096,8192,0x40);
    assert_eq!(heap.query_virtual(a),Some([a,a,1,4096,0x2000,0,0x20000]));
    assert_eq!(heap.query_virtual(a+4097),Some([a+4096,a,1,8192,0x1000,0x40,0x20000]));
    heap.virtual_free(a+4096,4096,false);
    assert_eq!(heap.query_virtual(a).unwrap()[3],8192);
    heap.virtual_free(a,0,true);
    assert_eq!(heap.query_virtual(a),None);
}
#[test]
fn fixed_arena_reservation_also_reports_only_committed_ram() {
    let mut heap=Heap::new(0x100000,0x40000);
    let a=heap.virtual_alloc(0x110000,0x10000,true,false).unwrap();
    assert!(!heap.external_virtual_block(a));
    assert_eq!(heap.physical_free_bytes(),0x40000);
    heap.virtual_alloc(a,4096,false,true).unwrap();
    assert_eq!(heap.physical_free_bytes(),0x3f000);
    heap.virtual_free(a,4096,false);
    assert_eq!(heap.physical_free_bytes(),0x40000);
    heap.virtual_free(a,0,true);
    assert_eq!(heap.free_bytes(),0x40000);
}
