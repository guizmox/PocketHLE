//! Standard desktop launcher entry point.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]
include!("desktop_main.rs");
