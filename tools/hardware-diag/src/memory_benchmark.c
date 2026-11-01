#include "imports.h"
typedef unsigned int U32;
typedef unsigned long long U64;
#define MEM_BYTES (2u*1024u*1024u)
#define MEM_WORDS (MEM_BYTES/4u)
#define MEM_GUARD 4u
#define MEM_CASES 10u
#define MEM_STEPS 16384u
#define MEM_SAMPLE_MS 300u
#define MEM_MAX_BLOCKS 65536u
static U32 out;
static volatile U32 *a,*b;
static volatile U32 sink;
static const char *const names[]={"read_16k","read_256k","read_2m","write_2m_arm","copy_2m_arm","copy_2m_memcpy","fill_2m_memset","stride_4k_2m","chase_16k","chase_2m"};
static const char *const units[]={"MiB/s","MiB/s","MiB/s","MiB/s","MiB/s","MiB/s","MiB/s","Maccess/s","ns/access","ns/access"};
static const U32 expected_read[]={0x2a858800u,0xc0588000u,0x02c40000u};
static U32 length(const char*s){U32 n=0;while(s[n])n++;return n;}
static void line(const char*s){U32 n;WriteFile(out,s,length(s),&n,0);WriteFile(out,"\r\n",2,&n,0);}
static void number(const char*key,U32 v){char buf[96],rev[12];U32 i=0,n=0;while(key[i]){buf[i]=key[i];i++;}do{rev[n++]=(char)('0'+v%10);v/=10;}while(v);while(n)buf[i++]=rev[--n];buf[i]=0;line(buf);}
static U32 value(U32 i){return i*1664525u+1013904223u;}
static void guards(volatile U32*p){U32 i;for(i=0;i<MEM_GUARD;i++){p[i]=0xd00dfeedu;p[MEM_GUARD+MEM_WORDS+i]=0xd00dfeedu;}}
static U32 guards_ok(volatile U32*p){U32 i;for(i=0;i<MEM_GUARD;i++)if(p[i]!=0xd00dfeedu||p[MEM_GUARD+MEM_WORDS+i]!=0xd00dfeedu)return 0;return 1;}
static void prepare(U32 id){U32 i,n=id==8?4096u:MEM_WORDS;for(i=0;i<MEM_WORDS;i++){a[i]=value(i);b[i]=0;}if(id>=8)for(i=0;i<n;i++)a[i]=value(i)&(n-1);}
/* Volatile accesses and no builtins keep ARM loads/stores in the ARM cases.
 * Setup, full output checks and report I/O are outside the measured interval. */
__attribute__((noinline)) static U32 block(U32 id){U32 i,j,n,sum=0,index=0;
 if(id<3){n=id==0?4096u:id==1?65536u:MEM_WORDS;for(i=0;i<n;i++)sum+=a[i];sink=sum;return sum==expected_read[id];}
 if(id==3){for(i=0;i<MEM_WORDS;i++)b[i]=0x13579bdfu;return 1;}
 if(id==4){for(i=0;i<MEM_WORDS;i++)b[i]=a[i];return 1;}
 if(id==5){WinCeMemcpy((void*)b,(const void*)a,MEM_BYTES);return 1;}
 if(id==6){WinCeMemset((void*)b,0x5a,MEM_BYTES);return 1;}
 if(id==7){for(j=0;j<32;j++)for(i=0;i<MEM_WORDS;i+=1024u)sum+=a[i];sink=sum;return sum==0xb657c000u;}
 for(i=0;i<MEM_STEPS;i++)index=a[index];sink=index;
 return index==(id==8?0u:0x14000u);
}
static U32 output_ok(U32 id,volatile U32*raw_a,volatile U32*raw_b){U32 i,want;
 if(!guards_ok(raw_a)||!guards_ok(raw_b))return 0;
 if(id>=3&&id<=6)for(i=0;i<MEM_WORDS;i++){
  want=id==3?0x13579bdfu:id==6?0x5a5a5a5au:value(i);if(b[i]!=want)return 0;
 }
 return 1;
}
static U32 rate(U32 id,U32 blocks,U32 ms){U64 n,d,q;
 if(!ms||!blocks)return 0;
 if(id<=6){U32 bytes=id==0?16384u:id==1?262144u:MEM_BYTES;n=(U64)bytes*blocks*1000000u;d=(U64)ms*1048576u;}
 else if(id==7){n=(U64)MEM_STEPS*blocks;d=ms;}
 else{n=(U64)ms*1000000000ull;d=(U64)MEM_STEPS*blocks;}
 q=__rt_udiv64by64(n,d);if(!q||q>0xffffffffull)return 0;return (U32)q;
}
__attribute__((section(".text.entry"))) U32 entry(void){U32 id,pass,start,elapsed,blocks,failed=0,bad,rates[3],t,i;
 volatile U32 *raw_a=0,*raw_b=0;
 out=CreateFileW(L"\\Flash Disk\\MEMORY.TXT",0x40000000,3,0,2,0,0);if(out==0xffffffff)return 1;
 line("HARDWARE DIAG Memory benchmark v1");
 line("ARM guest / wall time: emulator memory paths, NOT physical device RAM bandwidth.");
 line("3 x 300ms per case after warmup. Medians; fixed seeds and working sets.");
 line("MiB/s uses logical payload bytes, copies counted once; not estimated bus traffic.");
 line("Strided case reports Maccess/s; dependent chains report ns/access (lower is faster).");
 line("Setup/full output verification/report I/O excluded. Per-block read checks included.");
 line("Host optimization choices are not inferred by this guest. Relaunch after changes.");
 raw_a=VirtualAlloc(0,MEM_BYTES+8u*MEM_GUARD,0x3000,4);
 raw_b=VirtualAlloc(0,MEM_BYTES+8u*MEM_GUARD,0x3000,4);
 if(!raw_a||!raw_b){line("FAIL allocation: two 2MiB buffers required; scores suppressed.");failed=1;goto cleanup;}
 a=raw_a+MEM_GUARD;b=raw_b+MEM_GUARD;guards(raw_a);guards(raw_b);
 for(id=0;id<MEM_CASES;id++){
  number("MEM_CASE_ID=",id);line(names[id]);line(units[id]);bad=0;
  number("working_set_bytes=",id==0||id==8?16384u:id==1?262144u:MEM_BYTES);
  number("working_buffers=",id>=4&&id<=5?2:1);
  prepare(id);
  if(!block(id)||!output_ok(id,raw_a,raw_b)){line("MEM_CASE_RESULT FAIL warmup computation");failed=1;continue;}
  for(pass=0;pass<3;pass++){
   blocks=0;start=GetTickCount();
   do{if(!block(id)){bad=1;break;}blocks++;elapsed=GetTickCount()-start;if(elapsed>=0x80000000u){bad=1;break;}}
   while(elapsed<MEM_SAMPLE_MS&&blocks<MEM_MAX_BLOCKS);
   if(bad||!elapsed||!output_ok(id,raw_a,raw_b)){bad=1;break;}
   rates[pass]=rate(id,blocks,elapsed);if(!rates[pass]){bad=1;break;}
   number("mem_pass=",pass+1);number("mem_elapsed_ms=",elapsed);number("mem_blocks=",blocks);
   number("mem_sample_capped=",blocks==MEM_MAX_BLOCKS&&elapsed<MEM_SAMPLE_MS);
   number("mem_rate_milli=",rates[pass]);
  }
  if(bad){line("MEM_CASE_RESULT FAIL computation/clock/rate; score suppressed");failed=1;continue;}
  for(i=0;i<2;i++)for(t=0;t<2-i;t++)if(rates[t]>rates[t+1]){U32 swap=rates[t];rates[t]=rates[t+1];rates[t+1]=swap;}
  number("MEM_MEDIAN_MILLI=",rates[1]);line("MEM_CASE_RESULT PASS");
 }
 cleanup:
 if(raw_b&&!VirtualFree((void*)raw_b,0,0x8000)){line("FAIL VirtualFree B");failed=1;}
 if(raw_a&&!VirtualFree((void*)raw_a,0,0x8000)){line("FAIL VirtualFree A");failed=1;}
 line(failed?"MEMORY_RESULT FAIL":"MEMORY_RESULT PASS");CloseHandle(out);return failed?1:0;
}
