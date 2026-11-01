#include "imports.h"
typedef unsigned int U32;typedef unsigned short W16;
void __aeabi_memclr4(void*p,U32 n){volatile unsigned char*b=p;while(n--)*b++=0;}
static char report[8192];static U32 used,failed;
static void line(const char*s){while(*s)report[used++]=*s++;}
static void hex(U32 n){U32 i;line("0x");for(i=0;i<8;i++)report[used++]="0123456789abcdef"[(n>>(28-4*i))&15];}
static void check(const char*s,U32 ok){
#ifdef HARDWARE_DIAG
 hardware_diag_progress(s,ok);
#endif
line(ok?"PASS ":"FAIL ");line(s);if(!ok){failed++;line(" error=");hex(GetLastError());}line("\r\n");}
static U32 word(const unsigned char*p,U32 o){return p[o]|p[o+1]<<8|p[o+2]<<16|p[o+3]<<24;}
static U32 save(const W16*path,const void*p,U32 size){U32 n=0,h=CreateFileW(path,0x40000000,0,0,2,0,0),ok;if(h==0xffffffff)return 0;ok=WriteFile(h,p,size,&n,0)&&n==size;CloseHandle(h);return ok;}
static U32 same_bytes(const void*a,const void*b,U32 n){const unsigned char*x=a,*y=b;U32 i;for(i=0;i<n;i++)if(x[i]!=y[i])return 0;return 1;}
static U32 unhex(unsigned char c){if(c>='0'&&c<='9')return c-'0';if(c>='A'&&c<='F')return c-'A'+10;if(c>='a'&&c<='f')return c-'a'+10;return 256;}
static U32 field(const unsigned char*b,U32 start,U32 end,U32 index){while(index&&start<end){if(b[start++]==',')index--;}return !index&&start<end?b[start]:0;}
static void serial_gps(void){
 U32 h,other,n=0,t,used=0,start=0,i,j,checksum,good=1,gga=0,rmc=0,valid_rmc=0,errors=999,stat[3]={0};
 U32 dcb[7]={0},copy[7]={0},timeouts[5]={0xffffffffu,0,100,0,0},read_timeouts[5]={0};unsigned char bytes[1024]={0};
 h=CreateFileW(L"COM8:",0x80000000,0,0,3,0,0);check("com8.open",h!=0xffffffff);if(h==0xffffffff)return;
 other=CreateFileW(L"COM8:",0x80000000,0,0,3,0,0);check("com8.exclusive_share",other==0xffffffff&&GetLastError()==32);if(other!=0xffffffff)CloseHandle(other);
 check("com8.setup",SetupComm(h,4096,4096));
 dcb[0]=28;check("com8.get_dcb",GetCommState(h,dcb)&&dcb[0]==28);dcb[1]=4800;((unsigned char*)dcb)[18]=8;((unsigned char*)dcb)[19]=0;((unsigned char*)dcb)[20]=0;
 check("com8.configure_4800_8n1",SetCommState(h,dcb));copy[0]=28;check("com8.dcb_roundtrip",GetCommState(h,copy)&&same_bytes(dcb,copy,28));
 check("com8.timeouts",SetCommTimeouts(h,timeouts)&&GetCommTimeouts(h,read_timeouts)&&same_bytes(timeouts,read_timeouts,20));
 check("com8.reject_write_on_read_handle",!WriteFile(h,"X",1,&n,0)&&GetLastError()==5);
 t=GetTickCount();
 while(GetTickCount()-t<5000&&used+17<sizeof(bytes)&&(!gga||!rmc)){
  if(!ReadFile(h,bytes+used,17,&n,0)){good=0;break;}used+=n;if(!n){Sleep(25);continue;}
  for(i=start;i<used;i++)if(bytes[i]=='\n'){
   U32 end=i,star=start;while(star<end&&bytes[star]!='*')star++;
   if(bytes[start]!='$'||end<start+9||bytes[end-1]!='\r'||star+4!=end||unhex(bytes[star+1])>15||unhex(bytes[star+2])>15)good=0;
   else{checksum=0;for(j=start+1;j<star;j++)checksum^=bytes[j];if(checksum!=(unhex(bytes[star+1])<<4|unhex(bytes[star+2])))good=0;
    if(same_bytes(bytes+start,"$GPGGA,",7)){U32 quality=field(bytes,start,star,6);gga=1;if(quality<'0'||quality>'9')good=0;}
    if(same_bytes(bytes+start,"$GPRMC,",7)){U32 status=field(bytes,start,star,2);rmc=1;valid_rmc=status=='A';if(status!='A'&&status!='V')good=0;}
   }start=i+1;
  }
 }
 check("com8.fragmented_nmea_checksums_and_crlf",good&&gga&&rmc);
 check("com8.gga_and_rmc",gga&&rmc);line(valid_rmc?"COM8_POSITION AVAILABLE\r\n":"COM8_POSITION NO_FIX (NMEA status is still checked)\r\n");
 check("com8.save_nmea",save(L"\\Flash Disk\\COM8-NMEA.TXT",bytes,used));
 check("com8.clear_errors",ClearCommError(h,&errors,stat)&&errors==0&&stat[2]==0);
 check("com8.purge",PurgeComm(h,8));check("com8.close",CloseHandle(h));
 for(i=0;i<16;i++){h=CreateFileW(L"COM8:",0x80000000,0,0,3,0,0);if(h==0xffffffff||!CloseHandle(h))break;}
 check("com8.reopen_close_16",i==16);
}
__attribute__((section(".text.entry"))) U32 entry(void){
 unsigned char p[180]={0};U32 h,other,n,t,i,fix=0;
 line("HARDWARE DIAG GPS1 + COM8 NMEA test v3\r\n");
 h=CreateFileW(L"GPS1:",0x80000000,0,0,3,0,0);check("gps.open",h!=0xffffffff);if(h==0xffffffff)goto finish;
 other=CreateFileW(L"GPS1:",0x80000000,0,0,3,0,0);check("gps.exclusive_share",other==0xffffffff&&GetLastError()==32);if(other!=0xffffffff)CloseHandle(other);
 check("gps.read_packed_180",ReadFile(h,p,180,&n,0)&&n==180);
 check("gps.reject_short",!ReadFile(h,p,179,&n,0)&&n==0&&GetLastError()==87);
 check("gps.reject_null",!ReadFile(h,0,180,&n,0)&&GetLastError()==87);
 check("gps.reject_write_on_read_handle",!WriteFile(h,p,21,&n,0)&&GetLastError()==5);
 t=GetTickCount();
 do {if(!ReadFile(h,p,180,&n,0)){check("gps.poll_native_position",0);break;}fix=word(p,15)!=0;if(fix)break;Sleep(100);}while(GetTickCount()-t<30000);
 line(fix?"GPS_POSITION AVAILABLE\r\n":"GPS_POSITION NO_FIX (permission/service/provider may be unavailable)\r\n");
 line("latitude_e7=");hex(word(p,21));line(" longitude_e7=");hex(word(p,25));line(" horizontal_error_cm=");hex(word(p,37));line("\r\n");
 line(fix&&word(p,37)<10000?"COLORS_POSITION_ELIGIBLE YES\r\n":"COLORS_POSITION_ELIGIBLE NO\r\n");
 check("gps.save_snapshot",save(L"\\Flash Disk\\GPSTEST.BIN",p,180));
 check("gps.close",CloseHandle(h));h=0xffffffff;
 for(i=0;i<32;i++){h=CreateFileW(L"GPS1:",0x80000000,0,0,3,0,0);if(h==0xffffffff||!ReadFile(h,p,180,&n,0)||n!=180||!CloseHandle(h)){check("gps.read_reopen_close_32",0);break;}h=0xffffffff;}
 if(i==32)check("gps.read_reopen_close_32",1);
finish:
 if(h!=0xffffffff)CloseHandle(h);
 serial_gps();
 line(failed?"GPSTEST_RESULT FAIL\r\n":"GPSTEST_RESULT PASS\r\n");save(L"\\Flash Disk\\GPSTEST.TXT",report,used);
 MessageBoxW(0,failed?L"GPS test failed. See GPSTEST.TXT in Flash Disk. Enable GPS / host location and check OS permissions.":fix?L"GPS test passed, native position received. See GPSTEST.TXT and GPSTEST.BIN in Flash Disk.":L"GPS API test passed, but no location fix within 30 seconds. See GPSTEST.TXT; check OS location settings and retry outdoors on Android.",L"PocketHLE GPSTEST",0);
 return failed?1:0;
}
