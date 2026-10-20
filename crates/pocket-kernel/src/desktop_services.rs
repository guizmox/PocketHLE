//! Process-owned shell notifications and Winsock 1 host-lookup storage.
use std::collections::HashMap;
#[derive(Default)]
pub struct LegacyWinsock {
    pub startups: u32,
    /// One reusable guest HOSTENT buffer per guest thread, never host pointers.
    pub hostent_buffers: HashMap<usize, u32>,
}
#[derive(Clone, Default)]
pub struct NotificationIcon {
    pub callback: u32,
    pub image: Option<crate::imaging::Image>,
    pub tooltip: String,
}
