//! Per-process WinINet handles, backed by native HTTP clients. Bounded streaming,
//! cancellation and native certificate verification are transport requirements.
use std::{collections::{HashMap,VecDeque},sync::{Arc,Mutex,Condvar,OnceLock,RwLock}};
use serde::{Serialize,Deserialize};
pub type Result<T>=std::result::Result<T,u32>;
pub const BUFFER_LIMIT:usize=65536;
#[derive(Clone,Serialize,Deserialize,Debug)]
pub struct SessionSpec {pub agent:String,pub access:u32,pub proxy:String,pub bypass:String}
#[derive(Clone,Serialize,Deserialize,Debug)]
pub struct RequestSpec {pub server:String,pub port:u16,pub secure:bool,pub method:String,pub path:String,pub version:String,
    pub user:String,pub password:String,pub headers:Vec<(String,String)>,pub flags:u32}
#[derive(Clone,Serialize,Deserialize,Debug)]
pub struct ResponseHead {pub status:u32,pub version:String,pub reason:String,pub headers:Vec<(String,String)>}
pub trait Transfer:Send {fn head(&mut self)->Result<Option<ResponseHead>>;fn available(&mut self)->Result<(usize,bool)>;fn read(&mut self,out:&mut[u8])->Result<Option<usize>>;}
pub trait Client:Send+Sync {fn start(&self,spec:RequestSpec,body:Vec<u8>)->Result<Box<dyn Transfer>>;}
pub trait Backend:Send+Sync {fn open(&self,spec:&SessionSpec)->Result<Arc<dyn Client>>;}
static HOST:OnceLock<RwLock<Option<Arc<dyn Backend>>>>=OnceLock::new();
pub fn install_host(backend:Arc<dyn Backend>){*HOST.get_or_init(||RwLock::new(None)).write().unwrap()=Some(backend);}
fn host()->Option<Arc<dyn Backend>> {
    let slot=HOST.get_or_init(||RwLock::new(None));
    #[cfg(windows)]{let mut h=slot.write().unwrap();if h.is_none(){*h=Some(Arc::new(windows::WindowsBackend));}}
    slot.read().unwrap().clone()
}
pub struct Session {pub spec:SessionSpec,pub client:Arc<dyn Client>}
pub struct Connection {pub parent:u32,pub server:String,pub port:u16,pub user:String,pub password:String}
pub struct Request {pub parent:u32,pub client:Arc<dyn Client>,pub spec:RequestSpec,pub transfer:Option<Box<dyn Transfer>>,pub sending:Option<(usize,u32,u32)>}
pub enum Handle {Session(Session),Connection(Connection),Request(Request)}
impl Handle {fn parent(&self)->Option<u32>{match self{Self::Session(_)=>None,Self::Connection(v)=>Some(v.parent),Self::Request(v)=>Some(v.parent)}}}
pub struct State {pub handles:HashMap<u32,Handle>,next:u32,backend:Option<Arc<dyn Backend>>}
impl Default for State {fn default()->Self{Self::new(host())}}
impl State {
    fn new(backend:Option<Arc<dyn Backend>>)->Self{Self{handles:HashMap::new(),next:0xb7300000,backend}}
    pub fn with_backend(backend:Arc<dyn Backend>)->Self{Self::new(Some(backend))}
    pub fn open(&mut self,spec:SessionSpec)->Result<u32>{let client=self.backend.as_ref().ok_or(12004u32)?.open(&spec)?;self.insert(Handle::Session(Session{spec,client}))}
    pub fn insert(&mut self,h:Handle)->Result<u32>{
        if self.handles.len()>=512{return Err(8);}
        if matches!(h,Handle::Request(_))&&self.handles.values().filter(|h|matches!(h,Handle::Request(_))).count()>=64{return Err(8);}
        let id=self.next;self.next=self.next.checked_add(1).ok_or(8u32)?;self.handles.insert(id,h);Ok(id)
    }
    pub fn close(&mut self,id:u32)->bool {
        if !self.handles.contains_key(&id){return false;}
        let children:Vec<_>=self.handles.iter().filter(|(_,h)|h.parent()==Some(id)).map(|(id,_)|*id).collect();
        for child in children{self.close(child);}self.handles.remove(&id);true
    }
    pub fn request(&mut self,id:u32)->Result<&mut Request>{match self.handles.get_mut(&id){Some(Handle::Request(r))=>Ok(r),Some(_)=>Err(12018),None=>Err(6)}}
    pub fn close_all(&mut self){self.handles.clear();}
}
impl Drop for State{fn drop(&mut self){self.close_all();}}

/// A native worker may publish one response head and at most 64KiB of unread
/// body. Closing the guest request wakes a producer waiting on backpressure.
#[derive(Clone,Default)] pub struct Channel(Arc<(Mutex<Mailbox>,Condvar)>);
#[derive(Default)] struct Mailbox{head:Option<Result<ResponseHead>>,bytes:VecDeque<u8>,end:Option<Result<()>>,cancelled:bool}
impl Channel {
    pub fn cancelled(&self)->bool{self.0.0.lock().unwrap().cancelled}
    pub fn publish(&self,head:ResponseHead){let mut m=self.0.0.lock().unwrap();if !m.cancelled{m.head=Some(Ok(head));self.0.1.notify_all();}}
    pub fn push(&self,mut bytes:&[u8])->Result<()> {
        let(m,c)=(&self.0.0,&self.0.1);let mut guard=m.lock().unwrap();
        while !bytes.is_empty(){while guard.bytes.len()>=BUFFER_LIMIT&&!guard.cancelled{guard=c.wait(guard).unwrap();}
            if guard.cancelled{return Err(12017);}let n=bytes.len().min(BUFFER_LIMIT-guard.bytes.len());guard.bytes.extend(&bytes[..n]);bytes=&bytes[n..];c.notify_all();}
        Ok(())
    }
    pub fn finish(&self,result:Result<()>){let mut m=self.0.0.lock().unwrap();if m.head.is_none(){m.head=Some(Err(result.as_ref().err().copied().unwrap_or(12019)));}m.end=Some(result);self.0.1.notify_all();}
    pub fn cancel(&self){let mut m=self.0.0.lock().unwrap();m.cancelled=true;m.bytes.clear();m.head=None;m.end=Some(Err(12017));self.0.1.notify_all();}
    pub fn transfer(&self,cancel:impl FnOnce()+Send+'static)->Box<dyn Transfer>{Box::new(ChannelTransfer{channel:self.clone(),cancel:Some(Box::new(cancel))})}
}
struct ChannelTransfer{channel:Channel,cancel:Option<Box<dyn FnOnce()+Send>>}
impl Transfer for ChannelTransfer {
    fn head(&mut self)->Result<Option<ResponseHead>>{let m=self.channel.0.0.lock().unwrap();match &m.head{Some(Ok(h))=>Ok(Some(h.clone())),Some(Err(e))=>Err(*e),None=>Ok(None)}}
    fn available(&mut self)->Result<(usize,bool)>{let m=self.channel.0.0.lock().unwrap();if m.bytes.is_empty(){if let Some(Err(e))=m.end{return Err(e);}}Ok((m.bytes.len(),m.end.is_some()))}
    fn read(&mut self,out:&mut[u8])->Result<Option<usize>>{let mut m=self.channel.0.0.lock().unwrap();let n=out.len().min(m.bytes.len());
        for b in &mut out[..n]{*b=m.bytes.pop_front().unwrap();}self.channel.0.1.notify_all();
        if n!=0||out.is_empty(){return Ok(Some(n));}match m.end{Some(Ok(()))=>Ok(Some(0)),Some(Err(e))=>Err(e),None=>Ok(None)}
    }
}
impl Drop for ChannelTransfer {fn drop(&mut self){self.channel.cancel();if let Some(cancel)=self.cancel.take(){cancel();}}}
#[cfg(windows)] mod windows;

#[cfg(test)] mod tests {
 use super::*;
 fn head()->ResponseHead{ResponseHead{status:404,version:"HTTP/1.1".into(),reason:"Not Found".into(),headers:vec![]}}
 #[test] fn fragmented_stream_distinguishes_pending_eof_and_late_error(){
  let c=Channel::default();let mut t=c.transfer(||{});assert!(t.head().unwrap().is_none());assert_eq!(t.read(&mut[0;4]),Ok(None));
  c.publish(head());assert_eq!(t.head().unwrap().unwrap().status,404);c.push(b"abc").unwrap();c.finish(Err(12030));
  assert_eq!(t.available(),Ok((3,true)));let mut out=[0;8];assert_eq!(t.read(&mut out),Ok(Some(3)));assert_eq!(&out[..3],b"abc");assert_eq!(t.read(&mut out),Err(12030));
 }
 #[test] fn closing_wakes_bounded_producer_and_cancels_once(){
  let c=Channel::default();let count=Arc::new(std::sync::atomic::AtomicUsize::new(0));let n=count.clone();let t=c.transfer(move||{n.fetch_add(1,std::sync::atomic::Ordering::SeqCst);});
  c.push(&vec![7;BUFFER_LIMIT]).unwrap();let producer=c.clone();let (tx,rx)=std::sync::mpsc::channel();
  let worker=std::thread::spawn(move||{tx.send(producer.push(b"more")).unwrap();});
  assert!(rx.recv_timeout(std::time::Duration::from_millis(10)).is_err());drop(t);
  assert_eq!(rx.recv_timeout(std::time::Duration::from_secs(1)).unwrap(),Err(12017));worker.join().unwrap();assert!(c.cancelled());assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst),1);
 }
 #[test] fn completed_stream_and_empty_read(){let c=Channel::default();let mut t=c.transfer(||{});assert_eq!(t.read(&mut[]),Ok(Some(0)));c.publish(head());c.finish(Ok(()));assert_eq!(t.available(),Ok((0,true)));assert_eq!(t.read(&mut[0;1]),Ok(Some(0)));}
}
