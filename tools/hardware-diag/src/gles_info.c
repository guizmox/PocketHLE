#include "imports.h"
typedef unsigned int U32;
typedef unsigned short W16;
static U32 report;
void __aeabi_memclr4(void*p,U32 n){volatile unsigned char*b=p;while(n--)*b++=0;}
static U32 len(const char*s){U32 n=0;while(s[n]&&n<32768)n++;return n;}
static void raw(const char*s,U32 n){U32 done;WriteFile(report,s,n,&done,0);}
static void line(const char*s){raw(s,len(s));raw("\r\n",2);}
static void item(const char*name,const char*s){U32 n=0,start=0;line(name);if(!s){line("  unavailable");return;}while(s[n]&&n<32768){n++;if(n-start==76){raw(s+start,n-start);line("");start=n;}}if(n>start){raw(s+start,n-start);line("");}}
static void number(const char*name,int value){char b[32],r[12];U32 n=0,i=0,v;if(value<0){b[i++]='-';v=0u-(U32)value;}else v=(U32)value;do{r[n++]=(char)('0'+v%10);v/=10;}while(v);while(n)b[i++]=r[--n];b[i]=0;raw(name,len(name));raw(" = ",3);line(b);}
static U32 proc(U32 h,U32 m,U32 w,U32 l){return DefWindowProcW(h,m,w,l);}
#define LOAD(ret,name,args) ret (*name) args = (void*)GetProcAddressA(module,#name)
__attribute__((section(".text.entry"))) U32 entry(void){U32 module,display,context=0,surface=0,config=0,window=0,failed=0;int major=0,minor=0,num=0,v[4],i;unsigned char pixel[4]={0};
 struct {U32 style;void*proc;int cls_extra,wnd_extra;U32 instance,icon,cursor,brush;const W16*menu,*name;} wc={0};
 static const int attrs[]={0x3033,4,0x3038}; /* EGL_SURFACE_TYPE = WINDOW_BIT */
 static const int caps[]={0x0d31,0x0d32,0x0d33,0x0d36,0x0d38,0x0d39,0x84e2,0x80e8,0x80e9,0x0d3a,0x0d52,0x0d53,0x0d54,0x0d55,0x0d56,0x0d57};
 static const char*capnames[]={"GL_MAX_LIGHTS","GL_MAX_CLIP_PLANES","GL_MAX_TEXTURE_SIZE","GL_MAX_MODELVIEW_STACK_DEPTH","GL_MAX_PROJECTION_STACK_DEPTH","GL_MAX_TEXTURE_STACK_DEPTH","GL_MAX_TEXTURE_UNITS","GL_MAX_ELEMENTS_VERTICES","GL_MAX_ELEMENTS_INDICES","GL_MAX_VIEWPORT_DIMS.width","GL_RED_BITS","GL_GREEN_BITS","GL_BLUE_BITS","GL_ALPHA_BITS","GL_DEPTH_BITS","GL_STENCIL_BITS"};
 static const int conf[]={0x3024,0x3023,0x3022,0x3021,0x3025,0x3026,0x3032,0x3031};
 static const char*cnames[]={"EGL_RED_SIZE","EGL_GREEN_SIZE","EGL_BLUE_SIZE","EGL_ALPHA_SIZE","EGL_DEPTH_SIZE","EGL_STENCIL_SIZE","EGL_SAMPLE_BUFFERS","EGL_SAMPLES"};
 static const int vertices[]={-50000,-45000,0,50000,-45000,0,0,50000,0};
 static const unsigned char colors[]={255,60,40,255,40,255,60,255,60,80,255,255};
 report=CreateFileW(L"\\Flash Disk\\OGLESTEST.TXT",0x40000000,3,0,2,0,0);if(report==0xffffffff)return 1;
 line("HARDWARE DIAG OpenGL ES information v1");line("Guest-visible controller and context; this is not a host-driver capability dump.");
 module=LoadLibraryW(L"libGLES_CL.dll");if(!module){line("FAIL GLES library unavailable");CloseHandle(report);return 1;}
 {LOAD(U32,eglGetDisplay,(U32));LOAD(U32,eglInitialize,(U32,int*,int*));LOAD(U32,eglChooseConfig,(U32,const int*,U32*,int,int*));LOAD(U32,eglCreateWindowSurface,(U32,U32,U32,const int*));LOAD(U32,eglCreateContext,(U32,U32,U32,const int*));LOAD(U32,eglMakeCurrent,(U32,U32,U32,U32));LOAD(U32,eglSwapBuffers,(U32,U32));LOAD(U32,eglDestroySurface,(U32,U32));LOAD(U32,eglDestroyContext,(U32,U32));LOAD(U32,eglTerminate,(U32));LOAD(U32,eglGetError,(void));LOAD(const char*,eglQueryString,(U32,int));LOAD(U32,eglGetConfigAttrib,(U32,U32,int,int*));
 LOAD(const char*,glGetString,(U32));LOAD(void,glGetIntegerv,(U32,int*));LOAD(U32,glGetError,(void));LOAD(void,glViewport,(int,int,int,int));LOAD(void,glClearColorx,(int,int,int,int));LOAD(void,glClear,(U32));LOAD(void,glEnableClientState,(U32));LOAD(void,glVertexPointer,(int,U32,int,const void*));LOAD(void,glColorPointer,(int,U32,int,const void*));LOAD(void,glDrawArrays,(U32,int,int));LOAD(void,glReadPixels,(int,int,int,int,U32,U32,void*));
 if(!eglGetDisplay||!eglInitialize||!eglChooseConfig||!eglCreateWindowSurface||!eglCreateContext||!eglMakeCurrent||!eglSwapBuffers||!eglDestroySurface||!eglDestroyContext||!eglTerminate||!glGetString||!glGetIntegerv||!glGetError||!glViewport||!glClearColorx||!glClear||!glEnableClientState||!glVertexPointer||!glColorPointer||!glDrawArrays||!glReadPixels){line("FAIL required EGL/GLES entry point unavailable");failed=1;goto finish;}
 display=eglGetDisplay(0);if(!display||!eglInitialize(display,&major,&minor)){line("FAIL eglInitialize");failed=1;goto finish;}number("EGL major",major);number("EGL minor",minor);
 if(eglQueryString){item("EGL_VENDOR",eglQueryString(display,0x3053));item("EGL_VERSION",eglQueryString(display,0x3054));item("EGL_EXTENSIONS",eglQueryString(display,0x3055));}else line("EGL query strings unavailable");
 if(!eglChooseConfig(display,attrs,&config,1,&num)||!num){line("FAIL eglChooseConfig");failed=1;goto terminate;}
 wc.proc=proc;wc.name=L"HardwareDiagGLES";wc.instance=0x10000;RegisterClassW(&wc);window=CreateWindowExW(0,wc.name,L"HARDWARE DIAG - OpenGL ES",0x10000000,0,0,320,240,0,0,wc.instance,0);
 surface=eglCreateWindowSurface(display,config,window,0);context=eglCreateContext(display,config,0,0);if(!window||!surface||!context||!eglMakeCurrent(display,surface,surface,context)){line("FAIL EGL surface/context/current");failed=1;goto cleanup;}
 item("GL_VENDOR",glGetString(0x1f00));item("GL_RENDERER",glGetString(0x1f01));item("GL_VERSION",glGetString(0x1f02));item("GL_EXTENSIONS",glGetString(0x1f03));
 for(i=0;i<16;i++){v[0]=v[1]=0;glGetIntegerv(caps[i],v);if(!glGetError()){number(capnames[i],v[0]);if(i==9)number("GL_MAX_VIEWPORT_DIMS.height",v[1]);}else{raw(capnames[i],len(capnames[i]));line(" = unavailable (query rejected)");}}
 if(eglGetConfigAttrib)for(i=0;i<8;i++){v[0]=0;if(eglGetConfigAttrib(display,config,conf[i],v))number(cnames[i],v[0]);}

#ifdef HARDWARE_DIAG_MONOLITHIC
 {extern volatile U32 hardware_diag_visual;hardware_diag_visual=1;}
#endif
 glViewport(0,0,320,240);glClearColorx(0,0,0,65536);glClear(0x4000);glEnableClientState(0x8074);glEnableClientState(0x8076);glVertexPointer(3,0x140c,0,vertices);glColorPointer(4,0x1401,0,colors);glDrawArrays(4,0,3);glReadPixels(160,120,1,1,0x1908,0x1401,pixel);
 if(glGetError()||!(pixel[0]||pixel[1]||pixel[2])){line("FAIL triangle center readback");failed=1;}else line("PASS colored triangle center readback");
 if(!eglSwapBuffers(display,surface)){line("FAIL eglSwapBuffers");failed=1;}Sleep(2000);
cleanup:
#ifdef HARDWARE_DIAG_MONOLITHIC
 {extern volatile U32 hardware_diag_visual;hardware_diag_visual=0;}
#endif
 eglMakeCurrent(display,0,0,0);if(context)eglDestroyContext(display,context);if(surface)eglDestroySurface(display,surface);if(window)DestroyWindow(window);
terminate:eglTerminate(display);
finish:if(eglGetError)number("EGL final error",eglGetError());}
 line(failed?"OGLESTEST_RESULT FAIL":"OGLESTEST_RESULT PASS");CloseHandle(report);FreeLibrary(module);return failed?1:0;
}
