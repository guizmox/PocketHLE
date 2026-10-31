#!/usr/bin/env python3
"""Compare the production indexed allocator with the original best-fit search.
No Unicorn/Cargo rebuild. Requires a C compiler (CC or cc). Run at repo root.
Oracle: PocketHLE b5ce281c3285c761947b69a38021d9823da5c295, exec.c.
"""
from pathlib import Path
import os,subprocess,sys,tempfile
root=Path(sys.argv[1]) if len(sys.argv)>1 else Path(__file__).resolve().parents[1]
source=(root/'vendor/unicorn-engine-sys/qemu/exec.c').read_text()
def extract(name):
 start=source.index(name+'(');start=source.rfind('\n',0,start)+1
 begin=source.index('{',start);end=begin+1;depth=1
 while depth:
  depth+=(source[end]=='{')-(source[end]=='}');end+=1
 return source[start:end]
legacy=r'''
static ram_addr_t legacy(struct uc_struct *uc, ram_addr_t size)
{
 RAMBlock *block,*next_block;
 ram_addr_t offset=RAM_ADDR_MAX,mingap=RAM_ADDR_MAX;
 assert(size!=0);
 if(QLIST_EMPTY_RCU(&uc->ram_list.blocks))return 0;
 if(!uc->ram_list.freed)return find_ram_offset_last(uc,size);
 RAMBLOCK_FOREACH(block) {
  ram_addr_t candidate,next=RAM_ADDR_MAX;
  candidate=block->offset+block->max_length;
  candidate=ROUND_UP(candidate,BITS_PER_LONG<<TARGET_PAGE_BITS);
  RAMBLOCK_FOREACH(next_block) {
   if(next_block->offset>=candidate)next=MIN(next,next_block->offset);
  }
  if(next-candidate>=size&&next-candidate<mingap) {
   offset=candidate;mingap=next-candidate;
  }
 }
 if(offset==RAM_ADDR_MAX)abort();
 return offset;
}
'''
preamble=r'''
#include <assert.h>
#include <stdbool.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <inttypes.h>
#include <time.h>
#ifndef BITS_PER_LONG
#define BITS_PER_LONG 64
#endif
#define TARGET_PAGE_BITS 12
#define RAM_ADDR_MAX UINT64_MAX
#define MIN(a,b) ((a)<(b)?(a):(b))
#define ROUND_UP(n,d) (((n)+(d)-1)&~((ram_addr_t)(d)-1))
#define QLIST_EMPTY_RCU(p) (!(p)->first)
#define RAMBLOCK_FOREACH(v) for((v)=uc->ram_list.blocks.first;(v);(v)=(v)->next)
typedef uint64_t ram_addr_t;
typedef struct RAMBlock {ram_addr_t offset,max_length;struct RAMBlock *next;} RAMBlock;
struct uc_struct {struct {bool freed;struct {RAMBlock *first;}blocks;RAMBlock *last_block;}ram_list;};
static bool fail_index;
static unsigned index_allocations;
static void *index_malloc(size_t bytes) {index_allocations++;return fail_index?NULL:malloc(bytes);}
'''
tests=r'''
static uint32_t seed=19;
static uint32_t random32(void){seed=seed*1664525u+1013904223u;return seed;}
static void link_blocks(struct uc_struct *uc,RAMBlock *blocks,unsigned *order,unsigned count) {
 uc->ram_list.blocks.first=count?&blocks[order[0]]:NULL;
 for(unsigned i=0;i<count;i++)blocks[order[i]].next=i+1<count?&blocks[order[i+1]]:NULL;
 uc->ram_list.last_block=count?&blocks[order[count-1]]:NULL;
}
int main(void) {
 const ram_addr_t unit=(ram_addr_t)BITS_PER_LONG<<TARGET_PAGE_BITS;
 struct uc_struct uc={0};RAMBlock blocks[128];unsigned order[128];
 assert(find_ram_offset(&uc,4096)==0&&index_allocations==0);
 blocks[0]=(RAMBlock){.offset=unit,.max_length=unit};order[0]=0;
 link_blocks(&uc,blocks,order,1);
 assert(find_ram_offset(&uc,4096)==2*unit&&index_allocations==0);
 uc.ram_list.freed=true;
 for(unsigned trial=0;trial<2000;trial++) {
  unsigned count=1+random32()%128;ram_addr_t at=(random32()%3)*unit;
  for(unsigned i=0;i<count;i++) {
   blocks[i]=(RAMBlock){.offset=at,.max_length=(1+random32()%4)*unit};
   at+=blocks[i].max_length+(random32()%8)*unit;order[i]=i;
  }
  for(unsigned i=count-1;i;i--){unsigned j=random32()%(i+1);unsigned t=order[i];order[i]=order[j];order[j]=t;}
  link_blocks(&uc,blocks,order,count);
  for(unsigned request=0;request<12;request++) {
   ram_addr_t size=4096+(random32()%300)*4096ull;
   ram_addr_t expected=legacy(&uc,size);
   assert(find_ram_offset(&uc,size)==expected);
   if(request==0) {fail_index=true;assert(find_ram_offset(&uc,size)==expected);fail_index=false;}
  }
 }
 /* Equal-size gaps retain original list-order tie breaking, not address order. */
 for(unsigned i=0;i<3;i++)blocks[i]=(RAMBlock){.offset=i*4*unit,.max_length=unit};
 order[0]=1;order[1]=0;order[2]=2;link_blocks(&uc,blocks,order,3);
 assert(legacy(&uc,4096)==5*unit);assert(find_ram_offset(&uc,4096)==5*unit);
 /* 64-bit offsets must be compared without subtraction/truncation. */
 blocks[0]=(RAMBlock){.offset=UINT64_C(0x100000000),.max_length=unit};
 blocks[1]=(RAMBlock){.offset=UINT64_C(0x8000000000),.max_length=unit};
 order[0]=1;order[1]=0;link_blocks(&uc,blocks,order,2);
 assert(find_ram_offset(&uc,4096)==legacy(&uc,4096));
 puts("PASS: 24000 randomized allocations, original tie breaking, 64-bit offsets, empty/no-unmap fast paths and allocation-failure fallback");
 /* Isolated search benchmark; not an end-to-end Unicorn or Q3CE benchmark. */
 unsigned n=6000;RAMBlock *many=calloc(n,sizeof(*many));assert(many);
 for(unsigned i=0;i<n;i++){many[i].offset=(ram_addr_t)i*2*unit;many[i].max_length=unit;many[i].next=i+1<n?&many[i+1]:NULL;}
 uc.ram_list.blocks.first=many;uc.ram_list.last_block=&many[n-1];
 clock_t start=clock();ram_addr_t old=legacy(&uc,4096);clock_t middle=clock();
 ram_addr_t updated=find_ram_offset(&uc,4096);clock_t end=clock();assert(old==updated);
 printf("Search-only microbenchmark (%u blocks): original %.3f ms, indexed %.3f ms\n",n,1000.0*(middle-start)/CLOCKS_PER_SEC,1000.0*(end-middle)/CLOCKS_PER_SEC);
 free(many);return 0;
}
'''
code=preamble+'\n'+extract('find_ram_offset_last')+'\n'+legacy+'\n#define malloc index_malloc\n'+extract('ram_block_offset_compare')+'\n'+extract('ram_block_next_offset')+'\n'+extract('find_ram_offset')+'\n#undef malloc\n'+tests
with tempfile.TemporaryDirectory(prefix='ram-offset-test-',dir=root) as temp:
 c=Path(temp)/'test.c';c.write_text(code)
 for bits in [32,64]:
  exe=Path(temp)/('test'+str(bits))
  subprocess.run([os.environ.get('CC','cc'),'-std=c11','-O2','-Wall','-Wextra','-Werror','-DBITS_PER_LONG='+str(bits),str(c),'-o',str(exe)],check=True)
  subprocess.run([str(exe)],check=True)
