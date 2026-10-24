//! One thread ("qs-dbus") for all of D-Bus: a small executor that hosts every service as a task
//! and one shared connection per bus, built without zbus' internal executor thread and ticked
//! here instead (upstream Quickshell has one QDBusConnection thread the same way).

use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::task::{Context, Poll, Wake, Waker};
use zbus::export::futures_core::Stream;

type BoxFuture = Pin<Box<dyn Future<Output = ()> + Send>>;

struct Task {
    future: Mutex<Option<BoxFuture>>,
    queued: AtomicBool,
}

impl Wake for Task {
    fn wake(self: Arc<Self>) {
        schedule(self);
    }
}

#[derive(Default)]
struct Ready {
    queue: VecDeque<Arc<Task>>,
    /// The thread's own waker while it waits for work.
    idle: Option<Waker>,
}

fn ready() -> &'static Mutex<Ready> {
    static READY: OnceLock<Mutex<Ready>> = OnceLock::new();
    READY.get_or_init(|| {
        std::thread::Builder::new().name("qs-dbus".into()).spawn(run).expect("spawn qs-dbus thread");
        Mutex::default()
    })
}

fn schedule(task: Arc<Task>) {
    if task.queued.swap(true, Ordering::AcqRel) {
        return;
    }
    let mut r = ready().lock().unwrap();
    r.queue.push_back(task);
    let idle = r.idle.take();
    drop(r);
    if let Some(w) = idle {
        w.wake();
    }
}

fn run() {
    // async-io's block_on drives the I/O reactor on this thread whenever no task is ready.
    zbus::block_on(std::future::poll_fn(|cx| {
        // Bounded so a busy task can't keep the reactor from running.
        for _ in 0..128 {
            let task = {
                let mut r = ready().lock().unwrap();
                match r.queue.pop_front() {
                    Some(t) => t,
                    None => {
                        r.idle = Some(cx.waker().clone());
                        return Poll::<()>::Pending;
                    }
                }
            };
            task.queued.store(false, Ordering::Release);
            let waker = Waker::from(task.clone());
            let mut slot = task.future.lock().unwrap();
            if let Some(f) = slot.as_mut()
                && f.as_mut().poll(&mut Context::from_waker(&waker)).is_ready() {
                    *slot = None;
                }
        }
        cx.waker().wake_by_ref();
        Poll::Pending
    }))
}

/// Runs `future` on the D-Bus thread. Never blocks; callable from any thread.
pub fn spawn(future: impl Future<Output = ()> + Send + 'static) {
    schedule(Arc::new(Task { future: Mutex::new(Some(Box::pin(future))), queued: AtomicBool::new(false) }));
}

/// The next item of a stream (what `StreamExt::next` does).
pub async fn next<S: Stream + Unpin>(stream: &mut S) -> Option<S::Item> {
    std::future::poll_fn(|cx| Pin::new(&mut *stream).poll_next(cx)).await
}

#[derive(Clone, Copy, PartialEq)]
enum Bus {
    Session,
    System,
}

enum Slot {
    Empty,
    Connecting(Vec<Waker>),
    Done(Result<zbus::Connection, String>),
}

fn slot(bus: Bus) -> &'static Mutex<Slot> {
    static SESSION: Mutex<Slot> = Mutex::new(Slot::Empty);
    static SYSTEM: Mutex<Slot> = Mutex::new(Slot::Empty);
    if bus == Bus::Session { &SESSION } else { &SYSTEM }
}

async fn connection(bus: Bus) -> zbus::Result<zbus::Connection> {
    let first = std::future::poll_fn(|cx| {
        let mut s = slot(bus).lock().unwrap();
        match &mut *s {
            Slot::Empty => {
                *s = Slot::Connecting(Vec::new());
                Poll::Ready(None)
            }
            Slot::Connecting(waiters) => {
                waiters.push(cx.waker().clone());
                Poll::Pending
            }
            Slot::Done(r) => Poll::Ready(Some(r.clone())),
        }
    })
    .await;
    let result = match first {
        Some(r) => r,
        None => {
            let builder = match bus {
                Bus::Session => zbus::connection::Builder::session(),
                Bus::System => zbus::connection::Builder::system(),
            };
            let r = match builder {
                Ok(b) => b.internal_executor(false).build().await.map_err(|e| e.to_string()),
                Err(e) => Err(e.to_string()),
            };
            if let Ok(conn) = &r {
                let executor = conn.executor().clone();
                spawn(async move {
                    loop {
                        executor.tick().await;
                    }
                });
            }
            let prev = std::mem::replace(&mut *slot(bus).lock().unwrap(), Slot::Done(r.clone()));
            if let Slot::Connecting(waiters) = prev {
                waiters.into_iter().for_each(Waker::wake);
            }
            r
        }
    };
    result.map_err(zbus::Error::Failure)
}

/// The shared session bus connection, connected on first use.
pub async fn session() -> zbus::Result<zbus::Connection> {
    connection(Bus::Session).await
}

/// The shared system bus connection, connected on first use.
pub async fn system() -> zbus::Result<zbus::Connection> {
    connection(Bus::System).await
}

/// An unbounded channel whose receiver is awaited on the D-Bus thread.
pub fn channel<T: Send>() -> (Sender<T>, Receiver<T>) {
    let shared = Arc::new(Mutex::new(Chan { items: VecDeque::new(), waker: None, senders: 1 }));
    (Sender(shared.clone()), Receiver(shared))
}

struct Chan<T> {
    items: VecDeque<T>,
    waker: Option<Waker>,
    senders: usize,
}

pub struct Sender<T>(Arc<Mutex<Chan<T>>>);
pub struct Receiver<T>(Arc<Mutex<Chan<T>>>);

impl<T> Sender<T> {
    pub fn send(&self, item: T) {
        let mut c = self.0.lock().unwrap();
        c.items.push_back(item);
        let w = c.waker.take();
        drop(c);
        if let Some(w) = w {
            w.wake();
        }
    }
}

impl<T> Clone for Sender<T> {
    fn clone(&self) -> Self {
        self.0.lock().unwrap().senders += 1;
        Self(self.0.clone())
    }
}

impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        let mut c = self.0.lock().unwrap();
        c.senders -= 1;
        let w = if c.senders == 0 { c.waker.take() } else { None };
        drop(c);
        if let Some(w) = w {
            w.wake();
        }
    }
}

impl<T> Receiver<T> {
    /// The next item, or `None` once every sender is gone.
    pub async fn recv(&mut self) -> Option<T> {
        std::future::poll_fn(|cx| {
            let mut c = self.0.lock().unwrap();
            if let Some(item) = c.items.pop_front() {
                return Poll::Ready(Some(item));
            }
            if c.senders == 0 {
                return Poll::Ready(None);
            }
            c.waker = Some(cx.waker().clone());
            Poll::Pending
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tasks_and_channel_run_on_one_thread() {
        let (tx, mut rx) = channel::<u32>();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        spawn(async move {
            let mut sum = 0;
            while let Some(v) = rx.recv().await {
                sum += v;
            }
            done_tx.send((sum, std::thread::current().name().map(String::from))).unwrap();
        });
        for v in 1..=10 {
            let tx = tx.clone();
            std::thread::spawn(move || tx.send(v)).join().unwrap();
        }
        drop(tx);
        let (sum, name) = done_rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        assert_eq!(sum, 55);
        assert_eq!(name.as_deref(), Some("qs-dbus"));
    }
}
