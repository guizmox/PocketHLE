#include "imports.h"
typedef unsigned int U32;
typedef unsigned short W16;
void __aeabi_memclr4(void*p,U32 n){volatile unsigned char*b=p;while(n--)*b++=0;}
#define BAD 0xffffffffu
#define R 0x80000000u
#define W 0x40000000u
static char report[4096];static U32 used,failures;
static void line(const char*s){while(*s)report[used++]=*s++;}
static void hex(U32 n){U32 i;line("0x");for(i=0;i<8;i++)report[used++]="0123456789abcdef"[(n>>(28-4*i))&15];}
static void check(const char*name,U32 ok){line(ok?"PASS ":"FAIL ");line(name);if(!ok){failures++;line(" error=");hex(GetLastError());}line("\r\n");}
static U32 control(U32 h,U32 fn,void*in,U32 il,void*out,U32 ol,U32*bytes){return DeviceIoControl(h,0x01010000u|(fn<<2),in,il,out,ol,bytes,0);}
static U32 write_all(U32 h,const void*p,U32 length){U32 n=0;return WriteFile(h,p,length,&n,0)&&n==length;}
static U32 save(const W16*name,const void*p,U32 length){U32 h=CreateFileW(name,W,0,0,2,0,0);U32 ok;if(h==BAD)return 0;ok=write_all(h,p,length);CloseHandle(h);return ok;}
static void le32(unsigned char*p,U32 n){p[0]=n;p[1]=n>>8;p[2]=n>>16;p[3]=n>>24;}
static U32 bitmap(const void*p){
 unsigned char header[66]={0};U32 h,ok;
 header[0]='B';header[1]='M';le32(header+2,66+153600);le32(header+10,66);le32(header+14,40);
 le32(header+18,320);le32(header+22,240u);header[26]=1;header[28]=16;le32(header+30,3);le32(header+34,153600);
 le32(header+54,0xf800);le32(header+58,0x07e0);le32(header+62,0x001f);
 h=CreateFileW(L"\\Flash Disk\\CAMTEST-preview.bmp",W,0,0,2,0,0);if(h==BAD)return 0;
 ok=write_all(h,header,66)&&write_all(h,p,153600);CloseHandle(h);return ok;
}
__attribute__((section(".text.entry"))) U32 entry(void){
 U32 h=BAD,bytes=0,format[4]={640,480,320,240},get[4]={0},bad[4]={640,480,321,240};
 U32 info[4]={320,240,0,10000},old_count,t;void *pixels=0;
 line("PocketHLE ARM CAM1 test v1\r\n");
 h=CreateFileW(L"CAM1:",R|W,0,0,3,0,0);check("camera.open",h!=BAD);if(h==BAD)goto finish;
 check("camera.stop_initial",control(h,2104,0,0,0,0,&bytes));
 check("camera.reject_non_multiple_of_8",!control(h,2101,bad,16,0,0,&bytes)&&GetLastError()==87);
 check("camera.set_format",control(h,2101,format,16,0,0,&bytes));
 check("camera.get_format",control(h,2102,0,0,get,16,&bytes)&&bytes==16&&get[0]==640&&get[1]==480&&get[2]==320&&get[3]==240);
 pixels=VirtualAlloc(0,460800,0x3000,4);check("camera.allocate_buffers",pixels!=0);if(!pixels)goto finish;
 check("camera.start",control(h,2103,0,0,0,0,&bytes));
 if(!control(h,2105,info,16,pixels,153600,&bytes)){check("camera.preview_first",0);goto finish;}
 check("camera.preview_first",bytes==153600&&info[2]!=0);old_count=info[2];t=GetTickCount();
 check("camera.preview_second",control(h,2105,info,16,pixels,153600,&bytes)&&bytes==153600&&info[2]!=old_count);
 check("camera.preview_rate_at_most_20fps",GetTickCount()-t>=40);
 check("camera.save_rgb565_bitmap",bitmap(pixels));
 info[0]=640;info[1]=480;
 if(control(h,2106,info,16,pixels,460800,&bytes)){
  check("camera.capture_i420",bytes==460800);check("camera.save_i420",save(L"\\Flash Disk\\CAMTEST-capture.i420",pixels,460800));
 }else check("camera.capture_i420",0);
 check("camera.unsupported_ioctl",!control(h,2107,0,0,0,0,&bytes)&&GetLastError()==50);
 check("camera.stop_final",control(h,2104,0,0,0,0,&bytes));
finish:
 if(h!=BAD){control(h,2104,0,0,0,0,&bytes);CloseHandle(h);}if(pixels)VirtualFree(pixels,0,0x8000);
 line(failures?"CAMTEST_RESULT FAIL\r\n":"CAMTEST_RESULT PASS\r\n");
 if(!save(L"\\Flash Disk\\CAMTEST.TXT",report,used))failures++;
 MessageBoxW(0,failures?L"Camera test failed. See CAMTEST.TXT in Flash Disk. Enable Camera hardware and check OS camera permissions.":L"Camera test passed. CAMTEST-preview.bmp and CAMTEST-capture.i420 are in Flash Disk.",L"PocketHLE CAMTEST",0);
 return failures?1:0;
}
