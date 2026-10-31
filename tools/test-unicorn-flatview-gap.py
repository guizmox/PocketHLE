#!/usr/bin/env python3
"""Compile isolated tests of the actual memory.c renderer and insertion helper.
Uses a C compiler; no Unicorn/Cargo build or third-party Python modules.
Dispatch is instrumented, not the real Unicorn radix tree. This is not a full
backend regression suite. Run from the repository root or pass its path.
"""
from pathlib import Path
import os, subprocess, sys, tempfile
root=Path(sys.argv[1]) if len(sys.argv)>1 else Path(__file__).resolve().parents[1]
text=(root/'vendor/unicorn-engine-sys/qemu/softmmu/memory.c').read_text()
def function(name):
    start=text.index(name+'(')
    start=text.rfind('\n',0,start)+1
    brace=text.index('{',start);depth=1;end=brace+1
    while depth:
        depth+=(text[end]=='{')-(text[end]=='}');end+=1
    return text[start:end]
preamble=r'''
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
typedef __int128 Int128;
typedef uint64_t hwaddr;
#define int128_make64(x) ((Int128)(uint64_t)(x))
#define int128_get64(x) ((uint64_t)(x))
#define int128_zero() ((Int128)0)
#define int128_2_64() (((Int128)1)<<64)
#define int128_gt(a,b) ((a)>(b))
#define int128_ge(a,b) ((a)>=(b))
#define int128_lt(a,b) ((a)<(b))
#define int128_nz(a) ((a)!=0)
#define int128_add(a,b) ((a)+(b))
#define int128_sub(a,b) ((a)-(b))
#define int128_addto(a,b) (*(a)+=(b))
#define int128_subfrom(a,b) (*(a)-=(b))
#define int128_min(a,b) ((a)<(b)?(a):(b))
#define int128_max(a,b) ((a)>(b)?(a):(b))
#define MAX(a,b) ((a)>(b)?(a):(b))
#define g_realloc realloc
#define QTAILQ_EMPTY(h) (!(h)->first)
#define QTAILQ_FIRST(h) ((h)->first)
#define QTAILQ_NEXT(v,f) ((v)->next)
#define QTAILQ_FOREACH(v,h,f) for((v)=(h)->first;(v);(v)=(v)->next)
typedef struct { Int128 start,size; } AddrRange;
static AddrRange addrrange_make(Int128 a,Int128 n) { return (AddrRange){a,n}; }
static Int128 addrrange_end(AddrRange a) { return a.start+a.size; }
static bool addrrange_intersects(AddrRange a,AddrRange b) { return a.start<addrrange_end(b)&&b.start<addrrange_end(a); }
static AddrRange addrrange_intersection(AddrRange a,AddrRange b) { Int128 s=int128_max(a.start,b.start);return addrrange_make(s,int128_min(addrrange_end(a),addrrange_end(b))-s); }
typedef struct MemoryRegion MemoryRegion;
typedef struct FlatView FlatView;
typedef struct MemoryListener MemoryListener;
typedef struct AddressSpace AddressSpace;
struct MemoryListener { void *region_add,*region_del,*region_nop;MemoryListener *next; };
struct uc_struct { MemoryRegion *system_memory;int snapshot_level;unsigned target_page_size;struct {AddressSpace *first;} address_spaces; };
struct MemoryRegion { bool ram,is_iommu,subpage,enabled,terminates,readonly;Int128 size;hwaddr addr;int priority;MemoryRegion *container,*next;struct {MemoryRegion *first;} subregions;struct uc_struct *uc; };
typedef struct { MemoryRegion *mr;hwaddr offset_in_region;AddrRange addr;bool readonly; } FlatRange;
typedef struct {MemoryRegion *mr;FlatView *fv;hwaddr offset_in_region,offset_within_address_space;Int128 size;bool readonly;} MemoryRegionSection;
typedef struct {unsigned additions,compactions,clears;bool compacted;MemoryRegionSection last;} Dispatch;
struct FlatView { unsigned ref;FlatRange *ranges;unsigned nr,nr_allocated;Dispatch *dispatch;MemoryRegion *root; };
struct AddressSpace {MemoryRegion *root;FlatView *current_map;AddressSpace *next;struct {MemoryListener *first;} listeners;};
static FlatView *address_space_to_flatview(AddressSpace *as) {return as->current_map;}
static MemoryRegionSection section_from_flat_range(FlatRange *r,FlatView *fv) {return (MemoryRegionSection){.mr=r->mr,.fv=fv,.offset_in_region=r->offset_in_region,.offset_within_address_space=int128_get64(r->addr.start),.size=r->addr.size,.readonly=r->readonly};}
static Dispatch *rebuild_dispatch;
static void address_space_dispatch_clear(Dispatch *d) {assert(d->compacted);d->clears++;rebuild_dispatch=d;}
static Dispatch *address_space_dispatch_new(struct uc_struct *uc,FlatView *fv) {(void)uc;(void)fv;rebuild_dispatch->compacted=false;return rebuild_dispatch;}
static void flatview_add_to_dispatch(struct uc_struct *uc,FlatView *fv,MemoryRegionSection *s) {(void)uc;assert(!fv->dispatch->compacted);fv->dispatch->additions++;fv->dispatch->last=*s;}
static void address_space_dispatch_compact(Dispatch *d) {d->compactions++;d->compacted=true;}
'''
tests=r'''
static void compare(FlatView *a,FlatView *b) {
 assert(a->nr==b->nr);
 for(unsigned i=0;i<a->nr;i++) {
  assert(a->ranges[i].mr==b->ranges[i].mr);
  assert(a->ranges[i].addr.start==b->ranges[i].addr.start);
  assert(a->ranges[i].addr.size==b->ranges[i].addr.size);
  assert(a->ranges[i].offset_in_region==b->ranges[i].offset_in_region);
  assert(a->ranges[i].readonly==b->ranges[i].readonly);
 }
}
int main(void) {
 struct uc_struct uc={.target_page_size=4096};
 MemoryRegion root={.enabled=true,.size=int128_2_64(),.uc=&uc};uc.system_memory=&root;
 Dispatch dispatch={.compacted=true};FlatView fast={.ref=2,.root=&root,.dispatch=&dispatch};
 AddressSpace as={.root=&root,.current_map=&fast};uc.address_spaces.first=&as;
 MemoryRegion leaves[1024];memset(leaves,0,sizeof(leaves));
 unsigned seed=71,order[1024];for(unsigned i=0;i<1024;i++)order[i]=i;
 for(unsigned i=1023;i;i--) {seed=seed*1664525u+1013904223u;unsigned j=seed%(i+1);unsigned t=order[i];order[i]=order[j];order[j]=t;}
 for(unsigned i=0;i<1024;i++) {
  MemoryRegion *mr=&leaves[i];*mr=(MemoryRegion){.ram=true,.enabled=true,.terminates=true,.readonly=(i%3==0),.size=4096,.addr=4096+8192u*order[i],.container=&root,.uc=&uc};
  mr->next=root.subregions.first;root.subregions.first=mr;
  assert(flatview_insert_ram_gap(&fast,mr));
  assert(dispatch.additions==(i+1)*(i+2)/2&&dispatch.compactions==i+1&&dispatch.clears==i+1);
  assert(dispatch.last.size==4096);
  FlatView oracle={0};render_memory_region(&oracle,&root,0,addrrange_make(0,int128_2_64()),false);
  compare(&fast,&oracle);free(oracle.ranges);
 }
 unsigned nr=fast.nr,additions=dispatch.additions;
 MemoryRegion candidate={.ram=true,.enabled=true,.terminates=true,.size=4096,.addr=0x1000000,.container=&root,.uc=&uc};
 #define REJECT(statement,restore) do {statement;assert(!flatview_insert_ram_gap(&fast,&candidate));restore;assert(fast.nr==nr&&dispatch.additions==additions);}while(0)
 REJECT(candidate.addr=leaves[0].addr,candidate.addr=0x1000000);
 REJECT(candidate.addr=leaves[0].addr-4096;candidate.size=8192,candidate.addr=0x1000000;candidate.size=4096);
 REJECT(candidate.addr=1,candidate.addr=0x1000000);
 REJECT(candidate.size=1,candidate.size=4096);
 REJECT(candidate.size=0,candidate.size=4096);
 REJECT(candidate.enabled=false,candidate.enabled=true);
 REJECT(candidate.ram=false,candidate.ram=true);
 REJECT(candidate.is_iommu=true,candidate.is_iommu=false);
 REJECT(candidate.subpage=true,candidate.subpage=false);
 REJECT(candidate.terminates=false,candidate.terminates=true);
 REJECT(candidate.priority=1,candidate.priority=0);
 REJECT(uc.snapshot_level=1,uc.snapshot_level=0);
 REJECT(fast.ref=3,fast.ref=2);
 REJECT(candidate.container=NULL,candidate.container=&root);
 REJECT(root.size=4096,root.size=int128_2_64());
 REJECT(root.enabled=false,root.enabled=true);
 REJECT(root.terminates=true,root.terminates=false);
 REJECT(root.addr=4096,root.addr=0);
 REJECT(fast.root=&candidate,fast.root=&root);
 REJECT(candidate.subregions.first=&leaves[0],candidate.subregions.first=NULL);
 AddressSpace other={0};REJECT(as.next=&other,as.next=NULL);
 MemoryListener listener={.region_add=(void*)1};REJECT(as.listeners.first=&listener,as.listeners.first=NULL);
 listener.region_add=NULL;listener.region_del=(void*)1;REJECT(as.listeners.first=&listener,as.listeners.first=NULL);
 listener.region_del=NULL;listener.region_nop=(void*)1;REJECT(as.listeners.first=&listener,as.listeners.first=NULL);
 /* A moved leaf must not leave its former range mapped. */
 hwaddr old_addr=leaves[0].addr;leaves[0].addr=0x2000000;
 assert(!flatview_insert_ram_gap(&fast,&leaves[0]));leaves[0].addr=old_addr;
 /* Parent readonly is inherited, as in the unmodified renderer. */
 root.readonly=true;candidate.addr=0x3000000;
 assert(flatview_insert_ram_gap(&fast,&candidate));assert(dispatch.last.readonly);
 /* Last aligned page below 2^64 remains representable. */
 MemoryRegion top=candidate;top.addr=UINT64_MAX-4095;
 assert(flatview_insert_ram_gap(&fast,&top));
 MemoryRegion overflow=candidate;overflow.addr=UINT64_MAX-4095;overflow.size=8192;
 assert(!flatview_insert_ram_gap(&fast,&overflow));
 free(fast.ranges);
 puts("PASS: 1024 randomized insertions match original recursive renderer; fallback guards, moved regions, readonly and 64-bit bounds");
 return 0;
}
'''
code=preamble+'\n'+function('flatview_insert')+'\n'+function('render_memory_region')+'\n'+function('flatview_insert_ram_gap')+'\n'+tests
with tempfile.TemporaryDirectory(prefix='flatview-test-',dir=root) as tmp:
 c=Path(tmp)/'test.c';exe=Path(tmp)/'test';c.write_text(code)
 subprocess.run([os.environ.get('CC','cc'),'-std=c11','-O2','-Wall','-Wextra','-Werror',str(c),'-o',str(exe)],check=True)
 subprocess.run([str(exe)],check=True)
