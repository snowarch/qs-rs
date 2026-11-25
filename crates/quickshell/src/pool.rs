//! Bounded worker pool ("qs-io") shared by async filesystem operations (`FileView`,
//! `FolderListModel`, etc.). Spawns at most `MAX_WORKERS` OS threads under burst load and reaps
//! idle workers after `IDLE_TIMEOUT`, leaving zero permanent idle threads at rest.

use std::collections::VecDeque;
use std::sync::{Condvar, Mutex, OnceLock};
use std::time::Duration;

pub const MAX_WORKERS: usize = 2;
const IDLE_TIMEOUT: Duration = Duration::from_secs(3);

type Job = Box<dyn FnOnce() + Send>;

struct State {
    queue: VecDeque<Job>,
    workers: usize,
    idle: usize,
}

struct Pool {
    state: Mutex<State>,
    wake: Condvar,
}

fn pool() -> &'static Pool {
    static POOL: OnceLock<Pool> = OnceLock::new();
    POOL.get_or_init(|| Pool {
        state: Mutex::new(State { queue: VecDeque::new(), workers: 0, idle: 0 }),
        wake: Condvar::new(),
    })
}

/// Enqueues `job` on the bounded `qs-io` worker pool.
pub fn submit(job: impl FnOnce() + Send + 'static) {
    let p = pool();
    let mut st = p.state.lock().unwrap();
    st.queue.push_back(Box::new(job));
    if st.idle == 0 && st.workers < MAX_WORKERS {
        st.workers += 1;
        let spawned = std::thread::Builder::new().name("qs-io".into()).spawn(worker_loop);
        if spawned.is_err() {
            st.workers -= 1;
        }
    }
    p.wake.notify_one();
}

/// Current number of live worker threads (for tests/diagnostics).
pub fn worker_count() -> usize {
    pool().state.lock().unwrap().workers
}

struct WorkerGuard;

impl Drop for WorkerGuard {
    fn drop(&mut self) {
        if let Ok(mut st) = pool().state.lock() {
            st.workers = st.workers.saturating_sub(1);
        }
    }
}

fn worker_loop() {
    let _guard = WorkerGuard;
    let p = pool();
    loop {
        let job = {
            let mut st = p.state.lock().unwrap();
            loop {
                if let Some(j) = st.queue.pop_front() {
                    break j;
                }
                st.idle += 1;
                let (next, res) = p.wake.wait_timeout(st, IDLE_TIMEOUT).unwrap();
                st = next;
                st.idle -= 1;
                if let Some(j) = st.queue.pop_front() {
                    break j;
                }
                if res.timed_out() {
                    return;
                }
            }
        };
        job();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn bounds_workers_under_burst() {
        let done = Arc::new(AtomicUsize::new(0));
        for _ in 0..24 {
            let d = Arc::clone(&done);
            submit(move || {
                std::thread::sleep(Duration::from_millis(5));
                d.fetch_add(1, Ordering::SeqCst);
            });
        }
        assert!(worker_count() <= MAX_WORKERS);
        let start = std::time::Instant::now();
        while done.load(Ordering::SeqCst) < 24 && start.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(done.load(Ordering::SeqCst), 24);
    }
}
