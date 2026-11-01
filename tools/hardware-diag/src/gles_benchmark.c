/* Deterministic GLES 1.1 cost probes. No game-specific path or host timing API. */
#include "imports.h"
typedef unsigned int U32;
typedef unsigned short W16;
#define FIXED 0x140c
#define TRIANGLES 4
#define MAX_TRIANGLES 8192
#define CASES 10
#define SAMPLE_MS 600
#define MAX_FRAMES 4096
#define BATCH_FRAMES 8
#define LOAD(ret,name,args) static ret (*name) args
#define RESOLVE(name) name=(void*)GetProcAddressA(module,#name);if(!name){line("FAIL missing " #name);failed=1;goto finish;}
LOAD(U32,eglGetDisplay,(U32));LOAD(U32,eglInitialize,(U32,int*,int*));
LOAD(U32,eglChooseConfig,(U32,const int*,U32*,int,int*));
LOAD(U32,eglCreateWindowSurface,(U32,U32,U32,const int*));LOAD(U32,eglCreateContext,(U32,U32,U32,const int*));
LOAD(U32,eglMakeCurrent,(U32,U32,U32,U32));LOAD(U32,eglSwapBuffers,(U32,U32));
LOAD(U32,eglDestroySurface,(U32,U32));LOAD(U32,eglDestroyContext,(U32,U32));LOAD(U32,eglTerminate,(U32));
LOAD(const char*,glGetString,(U32));LOAD(U32,glGetError,(void));LOAD(void,glFinish,(void));
LOAD(void,glViewport,(int,int,int,int));LOAD(void,glClearColorx,(int,int,int,int));LOAD(void,glClear,(U32));
LOAD(void,glEnable,(U32));LOAD(void,glDisable,(U32));LOAD(void,glEnableClientState,(U32));LOAD(void,glDisableClientState,(U32));
LOAD(void,glVertexPointer,(int,U32,int,const void*));LOAD(void,glTexCoordPointer,(int,U32,int,const void*));
LOAD(void,glColor4x,(int,int,int,int));LOAD(void,glDrawArrays,(U32,int,int));
LOAD(void,glMatrixMode,(U32));LOAD(void,glLoadIdentity,(void));LOAD(void,glRotatex,(int,int,int,int));
LOAD(void,glGenTextures,(int,U32*));LOAD(void,glBindTexture,(U32,U32));LOAD(void,glDeleteTextures,(int,const U32*));
LOAD(void,glTexImage2D,(U32,int,int,int,int,int,U32,U32,const void*));LOAD(void,glTexParameteri,(U32,U32,int));
LOAD(void,glTexEnvx,(U32,U32,int));LOAD(void,glBlendFunc,(U32,U32));LOAD(void,glReadPixels,(int,int,int,int,U32,U32,void*));
static U32 report,io_failed;
static int vertices[MAX_TRIANGLES*6],original[MAX_TRIANGLES*6],uv[12];
static unsigned char texture_pixels[64*64*4];
static const char *const names[CASES]={"geometry_512","geometry_2048","geometry_8192","draw_calls_128","matrix_animated","vertices_rebuilt","fill_solid","fill_texture_nearest","fill_texture_linear","fill_texture_blend"};
/* Pairwise comparisons: first three sweep geometry; cases 3..5 use case 0;
   fill cases use identical overlapping fullscreen geometry. */
static const U32 triangles[CASES]={512,2048,8192,512,512,512,32,32,32,32};
static void raw(const char*s,U32 n){U32 done=0;if(!WriteFile(report,s,n,&done,0)||done!=n)io_failed=1;}
static U32 length(const char*s){U32 n=0;while(s[n])n++;return n;}
static void line(const char*s){raw(s,length(s));raw("\r\n",2);}
static void number(const char*key,U32 value){char b[12],r[12];U32 n=0,i=0;do{r[n++]=(char)('0'+value%10);value/=10;}while(value);while(n)b[i++]=r[--n];raw(key,length(key));raw("=",1);raw(b,i);raw("\r\n",2);}
static void decimal3(const char*key,U32 value){char b[16],r[12];U32 v=value/1000,n=0,i=0;do{r[n++]=(char)('0'+v%10);v/=10;}while(v);while(n)b[i++]=r[--n];b[i++]='.';b[i++]=(char)('0'+value/100%10);b[i++]=(char)('0'+value/10%10);b[i++]=(char)('0'+value%10);raw(key,length(key));raw("=",1);raw(b,i);raw("\r\n",2);}
static void string(const char*key,const char*value){raw(key,length(key));raw("=",1);line(value?value:"unavailable");}
static U32 proc(U32 h,U32 m,U32 w,U32 l){return DefWindowProcW(h,m,w,l);}
static U32 median(U32 a,U32 b,U32 c){U32 t;if(a>b){t=a;a=b;b=t;}if(b>c){t=b;b=c;c=t;}if(a>b)b=a;return b;}
/* Integer log2 Q16. Normalize to Q15 so the square fits in 32 bits. */
static U32 log2q(U32 v){U32 n=0,x,frac=0,i;while((v>>n)>1)n++;x=n>15?v>>(n-15):v<<(15-n);for(i=0;i<16;i++){x=(x*x)>>15;if(x>=65536){x>>=1;frac|=1u<<(15-i);}}return (n<<16)|frac;}
static U32 geometric_rate(const U32 *rates){U32 i,sum=0,lo=1,hi=1,mid,target;for(i=0;i<CASES;i++){sum+=log2q(rates[i]);if(rates[i]>hi)hi=rates[i];}target=sum/CASES;while(lo<hi){mid=lo+(hi-lo+1)/2;if(log2q(mid)<=target)lo=mid;else hi=mid-1;}return lo;}
static void prepare(U32 c){U32 i,n=triangles[c];int x,y;for(i=0;i<n;i++){
 if(c>=6){/* 16 fullscreen quads = 16 writes per pixel. */
  static const int quad[12]={-65536,-65536,65536,-65536,-65536,65536,-65536,65536,65536,-65536,65536,65536};
  U32 j;for(j=0;j<6;j++)vertices[i*6+j]=quad[(i&1)*6+j];
 }else{/* Small visible nondegenerate triangles: fixed size, no culling/depth rejection. */
  x=-40000+(int)(i%32)*2450;y=-40000+(int)((__rt_udiv(n,i*37)+i/32)%32)*2450;
  vertices[i*6]=x;vertices[i*6+1]=y;vertices[i*6+2]=x+700;vertices[i*6+3]=y;vertices[i*6+4]=x;vertices[i*6+5]=y+1400;
 }}for(i=0;i<n*6;i++)original[i]=vertices[i];}
static void setup(U32 c,U32 texture){U32 i;static const int texcoords[12]={0,0,65536,0,0,65536,0,65536,65536,0,65536,65536};
 glDisable(0x0be2);glDisable(0x0b71);glDisable(0x0b44);glDisable(0x0b50);glDisable(0x0de1);glDisable(0x0bd0);glDisableClientState(0x8076);glDisableClientState(0x8078);
 glMatrixMode(0x1701);glLoadIdentity();glMatrixMode(0x1700);glLoadIdentity();glColor4x(65536,45000,18000,65536);
 glEnableClientState(0x8074);glVertexPointer(2,FIXED,0,vertices);
 if(c>=7){for(i=0;i<12;i++)uv[i]=texcoords[i];glEnable(0x0de1);glBindTexture(0x0de1,texture);glTexEnvx(0x2300,0x2200,0x2100);glTexParameteri(0x0de1,0x2801,c==7?0x2600:0x2601);glTexParameteri(0x0de1,0x2800,c==7?0x2600:0x2601);
 glEnableClientState(0x8078);glTexCoordPointer(2,FIXED,0,uv);
 if(c==9){glEnable(0x0be2);glBlendFunc(0x0302,0x0303);glColor4x(65536,45000,18000,32768);}}
}
static void frame(U32 c,U32 frame_index){U32 i,n=triangles[c];int delta=(int)(frame_index%17)*16;
 glClear(0x4000);
 if(c==4){glLoadIdentity();glRotatex((int)(frame_index%360)*65536,0,0,65536);}
 if(c==5){/* Regenerate every coordinate from immutable source, no accumulated drift. */for(i=0;i<n*6;i++)vertices[i]=original[i]+delta;}
 if(c>=6){for(i=0;i<16;i++){glVertexPointer(2,FIXED,0,vertices+i*12);glDrawArrays(TRIANGLES,0,6);}}
 else if(c==3){for(i=0;i<n;i+=4)glDrawArrays(TRIANGLES,(int)i*3,12);}
 else glDrawArrays(TRIANGLES,0,(int)n*3);

}
/* Complete each batch without presentation or framebuffer readback. */
static void complete_batch(void){glFinish();}
U32 entry(void){U32 module=0,display=0,context=0,surface=0,config=0,window=0,texture=0,failed=0,c,p,i,rates[CASES],total_start,clock_bad=0;int major=0,minor=0,count=0;unsigned char pixel[4];
 struct{U32 style;void*proc;int ce,we;U32 instance,icon,cursor,brush;const W16*menu,*name;}wc={0};
 static const int attrs[]={0x3033,4,0x3038};
 report=CreateFileW(L"\\Flash Disk\\OGLESBENCH.TXT",0x40000000,3,0,2,0,0);if(report==0xffffffff)return 1;
 line("HARDWARE DIAG OpenGLES cost benchmark v1");line("Resolution=320x240; fixed workloads; 3 x 600ms samples after warmup.");line("render_millifps excludes swaps/readback; glFinish completes every 8-frame batch.");line("Includes guest preparation, emulation, GL submission and rendering; NOT GPU-only time.");line("Global index: geometric mean of case rates / 1000; reference 100 = 100 render frames/s in every case.");line("Throughput includes completion overhead. Compare v1 at the same renderer/settings. No inferred optimization flags.");
 module=LoadLibraryW(L"libGLES_CL.dll");if(!module){line("FAIL GLES library unavailable");failed=1;goto finish;}
 RESOLVE(eglGetDisplay) RESOLVE(eglInitialize) RESOLVE(eglChooseConfig) RESOLVE(eglCreateWindowSurface) RESOLVE(eglCreateContext) RESOLVE(eglMakeCurrent) RESOLVE(eglSwapBuffers) RESOLVE(eglDestroySurface) RESOLVE(eglDestroyContext) RESOLVE(eglTerminate)
 RESOLVE(glGetString) RESOLVE(glGetError) RESOLVE(glFinish) RESOLVE(glViewport) RESOLVE(glClearColorx) RESOLVE(glClear) RESOLVE(glEnable) RESOLVE(glDisable) RESOLVE(glEnableClientState) RESOLVE(glDisableClientState) RESOLVE(glVertexPointer) RESOLVE(glTexCoordPointer) RESOLVE(glColor4x) RESOLVE(glDrawArrays) RESOLVE(glMatrixMode) RESOLVE(glLoadIdentity) RESOLVE(glRotatex) RESOLVE(glGenTextures) RESOLVE(glBindTexture) RESOLVE(glDeleteTextures) RESOLVE(glTexImage2D) RESOLVE(glTexParameteri) RESOLVE(glTexEnvx) RESOLVE(glBlendFunc) RESOLVE(glReadPixels)
 display=eglGetDisplay(0);if(!display||!eglInitialize(display,&major,&minor)){line("FAIL EGL initialize");failed=1;display=0;goto finish;}
 if(!eglChooseConfig(display,attrs,&config,1,&count)||!count){line("FAIL EGL config");failed=1;goto cleanup;}
 wc.proc=proc;wc.name=L"HardwareDiagGLESBench";wc.instance=0x10000;RegisterClassW(&wc);window=CreateWindowExW(0,wc.name,L"HARDWARE DIAG - OpenGLES Benchmark",0x10000000,0,0,320,240,0,0,wc.instance,0);
 if(window)surface=eglCreateWindowSurface(display,config,window,0);if(surface)context=eglCreateContext(display,config,0,0);
 if(!window||!surface||!context||!eglMakeCurrent(display,surface,surface,context)){line("FAIL EGL context");failed=1;goto cleanup;}
 string("GL_VENDOR",glGetString(0x1f00));string("GL_RENDERER",glGetString(0x1f01));string("GL_VERSION",glGetString(0x1f02));
 glViewport(0,0,320,240);glClearColorx(0,0,0,65536);
 for(i=0;i<sizeof(texture_pixels);i+=4){U32 k=i/4;texture_pixels[i]=((k%64)/8+(k/64)/8)&1?240:32;texture_pixels[i+1]=120;texture_pixels[i+2]=200;texture_pixels[i+3]=255;}
 glGenTextures(1,&texture);glBindTexture(0x0de1,texture);glTexParameteri(0x0de1,0x2802,0x812f);glTexParameteri(0x0de1,0x2803,0x812f);glTexImage2D(0x0de1,0,0x1908,64,64,0,0x1908,0x1401,texture_pixels);
 if(!texture||glGetError()){line("FAIL texture initialization");failed=1;goto cleanup;}
 {extern volatile U32 hardware_diag_visual;hardware_diag_visual=1;}
 total_start=GetTickCount();
 for(c=0;c<CASES;c++){U32 samples[3],elapsed,frames,start;hardware_diag_progress(names[c],2);string("TEST",names[c]);number("triangles_per_frame",triangles[c]);number("draw_calls_per_frame",c>=6?16:c==3?128:1);prepare(c);setup(c,texture);
 /* Preview and independent readback outside every measurement. */
 frame(c,0);if(c>=6){glReadPixels(160,120,1,1,0x1908,0x1401,pixel);if(!(pixel[0]||pixel[1]||pixel[2])){line("FAIL black fill readback");failed=1;break;}}
 if(!eglSwapBuffers(display,surface)||glGetError()){line("FAIL preview/render GL error");failed=1;break;}
 /* Short visible animation is outside the timing windows. */
 for(i=1;i<=8;i++){frame(c,i*9);if(!eglSwapBuffers(display,surface)){line("FAIL preview swap");failed=1;break;}}if(failed)break;
 /* Warm up the same workload, including guest transformations. */
 for(i=0;i<3;i++)frame(c,i);complete_batch();
 for(p=0;p<3;p++){start=GetTickCount();frames=0;do{for(i=0;i<BATCH_FRAMES;i++){frame(c,frames);frames++;}complete_batch();elapsed=GetTickCount()-start;}while(elapsed<SAMPLE_MS&&frames<MAX_FRAMES);
 if(!elapsed||elapsed>=0x80000000u){line("FAIL invalid/nonadvancing clock");clock_bad=failed=1;break;}
 /* Max 4096 frames: numerator fits U32. */samples[p]=__rt_udiv(elapsed,frames*1000000u);number("pass",p+1);number("frames",frames);number("elapsed_ms",elapsed);number("render_millifps",samples[p]);
 if(!eglSwapBuffers(display,surface)||glGetError()||!samples[p]){line("FAIL GL error or zero throughput");failed=1;break;}
 }if(failed)break;
 rates[c]=median(samples[0],samples[1],samples[2]);number("MEDIAN_RENDER_MILLIFPS",rates[c]);decimal3("Render frames/s",rates[c]);
 /* Check the product before using the 32-bit report field. */
 {U32 whole=rates[c]/1000,fraction=(rates[c]%1000)*triangles[c]/1000;
 if(whole<=__rt_udiv(triangles[c],0xffffffffu-fraction))number("MEDIAN_TRIANGLES_PER_SECOND",whole*triangles[c]+fraction);
 else line("MEDIAN_TRIANGLES_PER_SECOND=overflow (use rate and triangle count)");}
 number("MEDIAN_FRAME_US",__rt_udiv(rates[c],1000000000u));decimal3("Frame ms",__rt_udiv(rates[c],1000000000u));line("TEST_RESULT PASS");hardware_diag_progress(names[c],1);
 if(GetTickCount()-total_start>180000){line("FAIL benchmark exceeded 180s budget; global score suppressed");failed=1;break;}
 }
 if(!failed&&!clock_bad&&!io_failed){U32 score=geometric_rate(rates);number("GLOBAL_INDEX_MILLI",score);decimal3("GLOBAL INDEX",score);line("100000 index milli = reference index 100; higher is faster. Not a hardware rating.");}
cleanup:
 if(failed)line("TEST_RESULT FAIL (no global score)");
 if(context){if(texture)glDeleteTextures(1,&texture);eglMakeCurrent(display,0,0,0);eglDestroyContext(display,context);}if(surface)eglDestroySurface(display,surface);if(window)DestroyWindow(window);if(display)eglTerminate(display);
 {extern volatile U32 hardware_diag_visual;hardware_diag_visual=0;}
finish:if(io_failed)failed=1;line(failed?"OGLESBENCH_RESULT FAIL":"OGLESBENCH_RESULT PASS");CloseHandle(report);if(module)FreeLibrary(module);return failed||io_failed?1:0;
}
