//! Background jobs. In `sync` mode (tests) jobs run inline so results are deterministic.

use std::sync::mpsc::{Receiver, Sender, channel};

pub struct Worker<M: Send + 'static> {
    tx: Sender<M>,
    rx: Receiver<M>,
    sync: bool,
    in_flight: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    /// Sync mode: results are queued but not yet polled.
    queued: std::sync::atomic::AtomicBool,
}

impl<M: Send + 'static> Worker<M> {
    pub fn new(sync: bool) -> Self {
        let (tx, rx) = channel();
        Self {
            tx,
            rx,
            sync,
            in_flight: Default::default(),
            queued: Default::default(),
        }
    }

    pub fn is_sync(&self) -> bool {
        self.sync
    }

    pub fn spawn(&self, ctx: &egui::Context, job: impl FnOnce() -> M + Send + 'static) {
        use std::sync::atomic::Ordering;
        if self.sync {
            let _ = self.tx.send(job());
            self.queued.store(true, std::sync::atomic::Ordering::SeqCst);
            return;
        }
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        let fl = self.in_flight.clone();
        fl.fetch_add(1, Ordering::SeqCst);
        std::thread::spawn(move || {
            let m = job();
            let _ = tx.send(m);
            fl.fetch_sub(1, Ordering::SeqCst);
            ctx.request_repaint();
        });
    }

    /// Like `spawn` but on the bounded rayon pool (for many small jobs such as thumbnails).
    pub fn spawn_pooled(&self, ctx: &egui::Context, job: impl FnOnce() -> M + Send + 'static) {
        use std::sync::atomic::Ordering;
        if self.sync {
            let _ = self.tx.send(job());
            self.queued.store(true, std::sync::atomic::Ordering::SeqCst);
            return;
        }
        let tx = self.tx.clone();
        let ctx = ctx.clone();
        let fl = self.in_flight.clone();
        fl.fetch_add(1, Ordering::SeqCst);
        rayon::spawn(move || {
            let m = job();
            let _ = tx.send(m);
            fl.fetch_sub(1, Ordering::SeqCst);
            ctx.request_repaint();
        });
    }

    pub fn poll(&self) -> Vec<M> {
        self.queued
            .store(false, std::sync::atomic::Ordering::SeqCst);
        self.rx.try_iter().collect()
    }

    /// Sync mode: results are waiting for the next frame.
    pub fn has_queued(&self) -> bool {
        self.queued.load(std::sync::atomic::Ordering::SeqCst)
    }

    pub fn busy(&self) -> bool {
        self.in_flight.load(std::sync::atomic::Ordering::SeqCst) > 0
    }
}
