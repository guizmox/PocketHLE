//! Compact Framework IL execution and guest services for PocketHLE.

mod forms;
mod graphics;
pub mod host;
pub mod il;
mod metadata;
mod runtime;
mod sensors;
pub mod snapshot;
mod storage;
mod system;
pub mod vm;

pub use metadata::{
    entry_point, graphics_api, method, referenced_member, references_accelerometer, EntryPoint,
};
pub use runtime::{Session, StartupError};

mod resources;

mod fmod;
