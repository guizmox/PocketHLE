#include "imports.h"
typedef unsigned int U32;typedef unsigned short W16;
volatile U32 hardware_diag_visual;
U32 diag_ram_entry(void);U32 diag_vfs_entry(void);U32 diag_timing_entry(void);U32 diag_camera_entry(void);U32 diag_gps_entry(void);U32 diag_network_entry(void);U32 diag_vibration_entry(void);U32 diag_bluetooth_entry(void);U32 diag_gles_entry(void);U32 diag_integer_entry(void);U32 diag_floating_entry(void);
U32 diag_proctest_entry(void);U32 diag_procworker_entry(U32,U32,const W16*,U32);U32 diag_vfsworker_entry(void);U32 hardware_diag_lifecycle_entry(void);
void __aeabi_memclr(void*p,U32 n){volatile unsigned char*b=p;while(n--)*b++=0;}
void __aeabi_memclr4(void*p,U32 n){__aeabi_memclr(p,n);}void __aeabi_memclr8(void*p,U32 n){__aeabi_memclr(p,n);}
static U32 match(const W16 *s,const W16*p){if(!s)return 0;while(*p)if(*s++!=*p++)return 0;return 1;}
static const W16 *argument(const W16*s){while(*s&&*s!=' ')s++;while(*s==' ')s++;return s;}
const W16 *hardware_diag_command(void){const W16*s=GetCommandLineW();if(s&&match(s,L"--hd-"))return argument(s);return s;}
static U32 same_tail(const W16*s,const W16*t){U32 n=0,m=0,i;while(s&&s[n])n++;while(t[m])m++;if(n<m)return 0;for(i=0;i<m;i++)if(s[n-m+i]!=t[i])return 0;return 1;}
U32 hardware_diag_spawn(const W16*app,const W16*cmd,void*a,void*b,U32 inherit,U32 flags,void*env,void*cwd,void*start,void*info){W16 self[260],args[260];const W16*mode=0;U32 i=0,n=0;
 if(same_tail(app,L"proctest.exe"))mode=L"--hd-proctest ";else if(same_tail(app,L"procworker.exe"))mode=L"--hd-procworker ";else if(same_tail(app,L"vfsworker.exe"))mode=L"--hd-vfsworker ";
 if(same_tail(app,L"badproc.exe"))app=L"\\SD Card\\GZHD999989\\ram\\badproc.bin";
 if(!mode)return CreateProcessW(app,cmd,a,b,inherit,flags,env,cwd,start,info);
 if(!GetModuleFileNameW(0,self,260))return 0;while(mode[i])args[n++]=mode[i++];i=0;while(cmd&&cmd[i]&&n<259)args[n++]=cmd[i++];if(cmd&&cmd[i]){SetLastError(87);return 0;}args[n]=0;
 return CreateProcessW(self,args,a,b,inherit,flags,env,cwd,start,info);
}
U32 hardware_diag_load_fixture(const W16 *name){W16 p[260];const W16*base=L"\\SD Card\\GZHD999989\\ram\\";U32 i=0,n=0;if(!name)return LoadLibraryW(name);while(name[i]){if(name[i]=='\\'||name[i]=='/')return LoadLibraryW(name);i++;}if(i>200)return LoadLibraryW(name);i=0;while(base[i])p[n++]=base[i++];i=0;while(name[i])p[n++]=name[i++];p[n]=0;return LoadLibraryW(p);}
/* Each suite's mutable storage is reset before reuse. Initialized writable
   globals are rejected by the builder rather than silently reset incorrectly. */
#define RANGE(name) extern unsigned char diag_##name##_bss_start[],diag_##name##_bss_end[];
RANGE(ram) RANGE(vfs) RANGE(timing) RANGE(camera) RANGE(gps) RANGE(network) RANGE(vibration) RANGE(bluetooth) RANGE(gles) RANGE(integer) RANGE(floating)
#define SPAN(name) {diag_##name##_bss_start,diag_##name##_bss_end}
static unsigned char *ranges[][2]={SPAN(ram),SPAN(vfs),SPAN(timing),SPAN(camera),SPAN(gps),SPAN(network),SPAN(vibration),SPAN(bluetooth),SPAN(gles),SPAN(integer),SPAN(floating)};
static U32 (*const suites[])(void)={diag_ram_entry,diag_vfs_entry,diag_timing_entry,diag_camera_entry,diag_gps_entry,diag_network_entry,diag_vibration_entry,diag_bluetooth_entry,diag_gles_entry,diag_integer_entry,diag_floating_entry};
U32 hardware_diag_run(U32 index){unsigned char*p;if(index>=11)return 1;for(p=ranges[index][0];p<ranges[index][1];p++)*p=0;return suites[index]();}
U32 hardware_diag_auxiliary(U32 *handled,U32 instance,U32 previous,const W16*command,U32 show){const W16*s=GetCommandLineW();U32 i=0;*handled=1;
 /* Preserve the actual WinMain command-line ABI check despite private role prefixes. */
 if(match(s,L"--hd-proctest")||match(s,L"--hd-procworker")||match(s,L"--hd-vfsworker")||match(s,L"--hd-lifecycle")){if(!command)return 1;while(s[i]&&command[i]&&s[i]==command[i])i++;if(s[i]!=command[i])return 1;}
if(match(s,L"--hd-proctest"))return diag_proctest_entry();if(match(s,L"--hd-procworker"))return diag_procworker_entry(instance,previous,argument(command),show);if(match(s,L"--hd-vfsworker"))return diag_vfsworker_entry();if(match(s,L"--hd-lifecycle"))return hardware_diag_lifecycle_entry();if(match(s,L"--hd-integer"))return hardware_diag_run(9);if(match(s,L"--hd-floating"))return hardware_diag_run(10);*handled=0;return 0;}
