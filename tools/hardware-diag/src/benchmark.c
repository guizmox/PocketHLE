#include "imports.h"
typedef unsigned int U32;
static volatile U32 result_sink;
static U32 out;
void __aeabi_memclr4(void*p,U32 n){volatile unsigned char*b=p;while(n--)*b++=0;}
static U32 len(const char*s){U32 n=0;while(s[n])n++;return n;}
static void line(const char*s){U32 n;WriteFile(out,s,len(s),&n,0);WriteFile(out,"\r\n",2,&n,0);}
static void number(const char*name,U32 v){char b[96],r[12];U32 i=0,n=0,j;while(name[i]){b[i]=name[i];i++;}do{r[n++]=(char)('0'+v%10);v/=10;}while(v);for(j=0;j<n;j++)b[i++]=r[n-1-j];b[i]=0;line(b);}
__attribute__((noinline)) static U32 block(void){U32 i,x;
#ifdef FLOATING
 x=0x3f800000;
 for(i=0;i<16384;i++){x=__adds(x,0x3f000000);x=__muls(x,0x3f7ff972);}
#else
 x=0x12345678;for(i=0;i<16384;i++){x^=x<<13;x^=x>>17;x^=x<<5;x=x*1664525u+1013904223u;}
#endif
 result_sink=x;return x;}
__attribute__((section(".text.entry"))) U32 entry(void){U32 pass,blocks,start,elapsed,value=0,failed=0;
#ifdef FLOATING
 const unsigned short*file=L"\\Flash Disk\\FLOATING.TXT";
 const char*title="Floating point / single precision / WinCE runtime ABI";
 const U32 expected=FLOAT_EXPECTED;
#else
 const unsigned short*file=L"\\Flash Disk\\INTEGER.TXT";
 const char*title="Integer / uint32 xorshift + wrapping multiply/add";
 const U32 expected=INT_EXPECTED;
#endif
 out=CreateFileW(file,0x40000000,3,0,2,0,0);if(out==0xffffffff)return 1;
 line("HARDWARE DIAG CPU benchmark v1");line(title);line("Same device/build/backend/power settings required for comparison.");line("Three timed passes after warmup. Each block restarts from a fixed seed.");
 if(block()!=expected){line("FAIL warmup checksum");CloseHandle(out);return 1;}
 for(pass=0;pass<3;pass++){blocks=0;start=GetTickCount();do{value=block();if(value!=expected)failed=1;blocks++;elapsed=GetTickCount()-start;}while(elapsed<1000&&blocks<65536);
 number("pass=",pass+1);number("elapsed_ms=",elapsed);number("work_iterations=",blocks*16384u);if(elapsed){U32 iterations=blocks*16384u,q=__rt_udiv(elapsed,iterations),rem=iterations-q*elapsed;if(q<=4294966u)number("work_iterations_per_second=",q*1000u+__rt_udiv(elapsed,rem*1000u));else line("Rate exceeds 32-bit reporting range");}number("checksum_bits=",value);if(!elapsed){failed=1;line("FAIL clock did not advance");}
#ifdef HARDWARE_DIAG
 hardware_diag_progress("benchmark pass complete",!failed);
#endif
 }
 line(failed?"BENCHMARK_RESULT FAIL":"BENCHMARK_RESULT PASS");line("PASS verifies computation; elapsed_ms/work_iterations measures performance.");CloseHandle(out);return failed?1:0;
}
