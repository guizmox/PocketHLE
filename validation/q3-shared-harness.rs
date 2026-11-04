use pocket_core::{Emulator,kernel::{FrameHook,FrameAction,KernelState}};
use std::{time::{Instant,Duration},sync::{Arc,atomic::AtomicBool}};
#[path="/workspace/scratch/1a0f598492f0/gpu-x1-work/frontends/pocket-desktop/src/latest_frame.rs"] mod latest_frame;
struct Logger;
impl log::Log for Logger {fn enabled(&self,m:&log::Metadata)->bool{m.level()<=log::Level::Info}fn log(&self,r:&log::Record){if self.enabled(r.metadata()){eprintln!("[{} {}] {}",r.level(),r.target(),r.args());}}fn flush(&self){}}
static LOGGER:Logger=Logger;
struct Hook {start:Instant,last:Instant,counter:u64,shared:u64,cpu:u64,tx:latest_frame::Sender<pocket_core::winceapi::gles::SharedFrame>}
impl FrameHook for Hook {fn on_frame(&mut self,k:&mut KernelState)->FrameAction{
 if self.counter!=k.framebuffer.frame_counter {self.counter=k.framebuffer.frame_counter;
  if let Some(frame)=pocket_core::winceapi::gles::take_shared_window_frame(&k.framebuffer){self.shared+=1;let _=self.tx.send(frame);}else{self.cpu+=1;}
 }
 if self.last.elapsed()>=Duration::from_secs(5){self.last=Instant::now();eprintln!("Q3_SHARED_PROGRESS t={:.1} frames_shared={} frames_cpu={} mapped={} cpu_resident={} gpu_authority={}",self.start.elapsed().as_secs_f64(),self.shared,self.cpu,k.fb_mapped,k.framebuffer.cpu_resident(),k.framebuffer.gpu_source().is_some());}
 if self.start.elapsed().as_secs()>=120{k.should_stop=true;FrameAction::Stop}else{FrameAction::Continue}
}}
fn main()->anyhow::Result<()> {
 log::set_logger(&LOGGER).unwrap();log::set_max_level(log::LevelFilter::Info);
 let _ui=pocket_gles::gpu::Renderer::new(8,8).map_err(anyhow::Error::msg)?;
 let mut owner=unsafe{pocket_core::winceapi::gles::ExternalGpuContext::capture_current().map_err(anyhow::Error::msg)?};
 let source=owner.source();let (tx,rx)=latest_frame::channel();
 let worker=std::thread::spawn(move || -> anyhow::Result<(u64,u64)>{
  pocket_core::winceapi::gles::with_shared_context_source(Some(source),||{
   pocket_core::winceapi::gles::with_shared_frame_export(Arc::new(AtomicBool::new(true)),||{
    let root=std::fs::canonicalize("tmp/q3-11b-input/quake3_1.1b/extracted")?;
    let save=std::fs::canonicalize("tmp/deferred-gapi-build/q3-save")?;
    let mut e=Emulator::with_unicorn_cpu()?;e.set_screen_size(640,480);e.max_slices=0;e.instruction_budget_per_slice=1_000_000;
    e.load_pe(root.join("q3ce.exe"))?;e.set_pocket_pc_input(true);e.set_gpu_renderer(true);e.set_frame_rate_limit(Some(60));
    for prefix in ["\\Application\\","\\Program Files\\Game\\","\\Storage Card\\","\\SD Card\\"]{e.mount_read_only_dir(prefix,&root);}
    e.mount_isolated_save_dir("\\Program Files\\Game\\",&save);e.set_default_dir("\\Program Files\\Game\\");e.set_module_path("\\Program Files\\Game\\q3ce.exe");e.set_synthetic_message_budget(0);
    let now=Instant::now();let mut hook=Hook{start:now,last:now,counter:0,shared:0,cpu:0,tx};
    e.run_with_hook(&mut hook)?;
    let pending=e.process().unwrap().state.framebuffer.gpu_source().is_some();
    eprintln!("Q3_SHARED_FINAL shared={} cpu={} final_gpu_authority={pending}",hook.shared,hook.cpu);
    Ok((hook.shared,hook.cpu))
   })
  })
 });
 let mut consumed=0;
 while !worker.is_finished(){if let Ok(frame)=rx.try_recv(){unsafe{frame.with_texture(&mut owner,|_|()).map_err(anyhow::Error::msg)?;}consumed+=1;}else{std::thread::sleep(Duration::from_millis(1));}}
 let result=worker.join().expect("worker panic")?;assert!(result.0>100);
 eprintln!("Q3_SHARED_UI_CONSUMED {consumed}");Ok(())
}
