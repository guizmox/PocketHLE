from pathlib import Path
import os,subprocess
r=Path('.').resolve();out=r/'tmp/x1-app-build';deps=r/'build-small/debug/deps'
def block(s,tag):
 a=s.index(tag);start=s.index('{',a);depth=1;i=start+1
 while depth:
  if s[i]=='{':depth+=1
  elif s[i]=='}':depth-=1
  i+=1
 return s[a:i]
head='''use std::sync::{Arc,Mutex,atomic::{AtomicBool,AtomicU32,Ordering},mpsc::{Sender,Receiver,TryRecvError}};
use std::time::{Duration,Instant};
use pocket_core::kernel::{KernelState,FrameAction,FrameHook,InputEvent};
'''
parts=[]
for platform,hook in [('pocket-desktop','RunHook'),('pocket-android-jni','SessionHook')]:
 s=(r/'gpu-x1-work/frontends'/platform/'src/runner.rs').read_text();p=head+block(s,'pub struct FrameSnapshot')+'\n'+block(s,'impl FrameSnapshot')+'\nconst FRAME_PUSH_INTERVAL: Duration = Duration::from_millis(16);\n'+block(s,'fn presentation_phase(')+'\n'+block(s,'impl FrameHook for '+hook)
 if hook=='RunHook':
  p+='''
impl RunHook {fn new(frame_tx:Option<Sender<FrameSnapshot>>,_:Option<Receiver<()>>)->Self {Self {recording_audio:None,recording_audio_pending:false,colors_random_gps:None,refresh_colors_gps:false,gps_notice_tx:None,stop_all:Arc::new(AtomicBool::new(false)),foreground:Arc::new(AtomicU32::new(1)),pid:1,stopped_by_user:false,frame_send_failed:false,frame_tx,last_frame:0,saw_non_black:false,last_emit_at:None,scratch:vec![]}}}
struct GpsConfig; impl GpsConfig {fn simulated_gps_position(&self)->Option<(f64,f64)>{None}}
struct RunHook { recording_audio: Option<Arc<pocket_core::kernel::audio::OutputRecording>>, recording_audio_pending:bool,
 colors_random_gps:Option<GpsConfig>, refresh_colors_gps:bool, gps_notice_tx:Option<Sender<(f64,f64)>>,
 stop_all:Arc<AtomicBool>, foreground:Arc<AtomicU32>, pid:u32, stopped_by_user:bool,
 frame_send_failed:bool, frame_tx:Option<Sender<FrameSnapshot>>, last_frame:u64, saw_non_black:bool,
 last_emit_at:Option<Instant>, scratch:Vec<u8> }
impl RunHook {fn reap_children(&mut self){} fn run_pending_child(&mut self,_:&mut KernelState){} fn drain_input(&mut self,_:&mut std::collections::VecDeque<InputEvent>)->bool{false}}
'''
 else:
  p+='''
enum InputCommand {Input(InputEvent),Stop}
struct SessionState {audio:Mutex<Option<pocket_core::kernel::audio::AudioTap>>,latest_frame:Mutex<Option<FrameSnapshot>>} impl SessionState {fn new()->Self {Self {audio:Mutex::new(None),latest_frame:Mutex::new(None)}}}
struct SessionHook {stop_all:Arc<AtomicBool>, foreground:Arc<AtomicU32>, pid:u32,input_disconnected:bool,
 input_rx:Arc<Mutex<Receiver<InputCommand>>>,state:Arc<SessionState>,last_frame:u64,saw_non_black:bool,last_emit_at:Option<Instant>,scratch:Vec<u8>}
impl SessionHook {fn reap_children(&mut self){} fn run_pending_child(&mut self,_:&mut KernelState){} fn queue_input(&mut self,ev:InputEvent,_:&pocket_core::kernel::gps::Service,q:&mut std::collections::VecDeque<InputEvent>){q.push_back(ev);}}
fn push_frame(s: &Arc<SessionState>,f:FrameSnapshot){*s.latest_frame.lock().unwrap()=Some(f);}
impl SessionHook {fn new(state:Arc<SessionState>,input_rx:Receiver<InputCommand>)->Self {Self {stop_all:Arc::new(AtomicBool::new(false)),foreground:Arc::new(AtomicU32::new(1)),pid:1,input_disconnected:false,input_rx:Arc::new(Mutex::new(input_rx)),state,last_frame:0,saw_non_black:false,last_emit_at:None,scratch:vec![]}}}
'''
 if hook=='RunHook':
  p+='\nenum InputCommand {Stop}\n#[derive(Debug,Clone)]\n'+block(s,'pub struct Runner')+'\n'+block(s,'impl Default for Runner')+'\nimpl Runner {\n'+block(s,'pub fn new()')+'\n'+block(s,'pub fn set_shared_context_source(')+'\n'+block(s,'pub fn shared_export_policy(')+'\n'+block(s,'pub(crate) fn stop_shared_worker(')+'\n}\n'
 p+='\n'+block(s,'mod deferred_read_tests')
 parts.append('mod '+platform.replace('-','_')+' {\n'+p+'\n}')
p=out/'hook-boundaries.rs';p.write_text('\n'.join(parts))
a=[str(r/'toolchain/direct/bin/rustc'),'--edition=2021','--test','-C','linker=/usr/bin/gcc','-C','link-arg=-fuse-ld=bfd','-L','native='+str(r/'tmp/unicorn-current-build'),'--crate-name','hook_boundaries',str(p),'-A','dead_code','-A','unused_imports','-L','dependency='+str(out),'-L','dependency='+str(deps),'--extern','pocket_gles='+str(out/'libpocket_gles.rlib'),'--extern','pocket_core='+str(out/'libpocket_core.rlib'),'--extern','log='+str(sorted(deps.glob('liblog-*.rlib'))[0]),'-o',str(out/'hook-boundaries-tests')]
env=os.environ.copy();env['LD_LIBRARY_PATH']=str(r/'toolchain/direct/lib')
with (out/'hook-boundaries-build.log').open('w') as f:res=subprocess.run(a,env=env,stdout=f,stderr=f)
print('Actual FrameHook + FrameSnapshot extraction typecheck:',res.returncode)
if res.returncode:print((out/'hook-boundaries-build.log').read_text())
raise SystemExit(res.returncode)
