from pathlib import Path
import os,subprocess
r=Path('.').resolve();d=r/'tmp/x1-presenter-build';deps=r/'build-small/debug/deps';app=r/'tmp/x1-app-build';old=r/'tmp/gpu-profile-build'
# Keep the isolated harness tied to the real snapshot API, not a hand-written mirror.
snapshot=(r/'gpu-x1-work/frontends/pocket-desktop/src/runner.rs').read_text()
def snapshot_block(tag):
 start=snapshot.index(tag);i=snapshot.index('{',start)+1;depth=1
 while depth:
  if snapshot[i]=='{':depth+=1
  elif snapshot[i]=='}':depth-=1
  i+=1
 return snapshot[start:i]
harness='extern crate self as eframe;\npub use glow;\npub mod runner {use std::sync::Arc;\n#[derive(Debug,Clone)]\n'+snapshot_block('pub struct FrameSnapshot')+'\n'+snapshot_block('impl FrameSnapshot')+'\n}\n'
for name in ['smaa','reconstruction']:
 harness+='#[path="'+str(r/'gpu-x1-work/frontends/pocket-desktop/src'/ (name+'.rs'))+'"] pub mod '+name+';\n'
app_source=(r/'gpu-x1-work/frontends/pocket-desktop/src/app.rs').read_text()
def app_block(tag):
 start=app_source.index(tag);i=app_source.index('{',start)+1;depth=1
 while depth:
  if app_source[i]=='{':depth+=1
  elif app_source[i]=='}':depth-=1
  i+=1
 return app_source[start:i]
harness+='pub mod app {use crate::runner::FrameSnapshot;\n#[derive(Debug,Clone,Copy,PartialEq,Eq)]\n'+app_block('enum UpscaleFilter')+'\n'+app_block('impl UpscaleFilter')+'\n'+app_block('pub(crate) fn filtered_snapshot_image(')+'\npub struct Capture {upscale_filter:UpscaleFilter}\nimpl Capture {pub fn nearest()->Self {Self {upscale_filter:UpscaleFilter::Nearest}}\n'+app_block('fn filtered_frame_image(').replace('fn filtered_frame_image','pub fn filtered_frame_image',1)+'\n}\n}\n'
(d/'presenter.rs').write_text(harness)
env=os.environ.copy();env['LD_LIBRARY_PATH']=str(r/'toolchain/direct/lib')
a=[str(r/'toolchain/direct/bin/rustc'),'--edition=2021','--crate-name','presenter_x1','--test',str(d/'presenter.rs'),'-C','opt-level=1','-C','debuginfo=0','-C','linker=/usr/bin/gcc','-C','link-arg=-fuse-ld=bfd','-L','dependency='+str(deps),'-L','dependency='+str(app),'-L','dependency='+str(old),'-L','dependency='+str(r/'tmp/x1-xbrz-build'),'-L','native='+str(r/'tmp/unicorn-current-build')]
for name,path in [('image',sorted(deps.glob('libimage-*.rlib'))[0]),('glow',old/'libglow.rlib'),('xbrz',r/'tmp/x1-xbrz-build/libxbrz_actual.rlib'),('pocket_core',app/'libpocket_core.rlib'),('pocket_gles',app/'libpocket_gles.rlib'),('log',sorted(deps.glob('liblog-*.rlib'))[0])]:a+=['--extern',name+'='+str(path)]
a+=['-o',str(d/'presenter-tests')]
with (d/'build.log').open('w') as f:p=subprocess.run(a,env=env,stdout=f,stderr=f)
print('presenter compile',p.returncode)
if p.returncode:print((d/'build.log').read_text()[-5000:])
raise SystemExit(p.returncode)
