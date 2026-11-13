//! Compact Framework IL execution and guest services for PocketHLE.

mod forms;
mod graphics;
pub mod host;
pub mod il;
mod metadata;
mod runtime;
mod sensors;
mod storage;
mod system;
pub mod vm;

pub use metadata::{entry_point, method, referenced_member, EntryPoint};
pub use runtime::{Session, StartupError};
