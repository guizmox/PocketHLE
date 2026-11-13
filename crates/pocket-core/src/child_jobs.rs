//! Host thread lifecycle for child guest processes.
use pocket_kernel::handles::HandleTable;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;

pub struct ChildJob<T> {
    pub table: HandleTable,
    pub thread: JoinHandle<T>,
}

pub fn finish<T>(children: &mut Vec<ChildJob<T>>, stop_all: &AtomicBool) {
    for job in children.drain(..) {
        if !job.table.start_allowed() {
            job.table
                .terminate_remote_process(job.table.process_id(), 0xc0000001);
            job.table.allow_start();
        }
        if job.thread.join().is_err() {
            stop_all.store(true, Ordering::Release);
        }
    }
}

pub fn reap<T>(children: &mut Vec<ChildJob<T>>) {
    let mut index = 0;
    while index < children.len() {
        if children[index].thread.is_finished() {
            let job = children.swap_remove(index);
            if job.thread.join().is_err() {
                job.table.complete_process_exit(0xc0000005);
                job.table.set_exit(Some(0), 0xc0000005);
                job.table.mark_inactive();
            }
        } else {
            index += 1;
        }
    }
}
