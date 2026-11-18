use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

use bgp_api_core::ApiState;
use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::watch;

use crate::config::{CachePlan, GIBIBYTE};

#[derive(Debug)]
pub struct RuntimeWarmer {
    state: Arc<ApiState>,
    plan: CachePlan,
    running: AtomicBool,
    dataset_warmed: AtomicBool,
}

impl RuntimeWarmer {
    #[must_use]
    pub fn new(state: Arc<ApiState>, plan: CachePlan) -> Arc<Self> {
        Arc::new(Self {
            state,
            plan,
            running: AtomicBool::new(false),
            dataset_warmed: AtomicBool::new(false),
        })
    }

    pub fn spawn_start(self: &Arc<Self>) {
        if self.running.swap(true, Ordering::AcqRel) {
            return;
        }
        let me = Arc::clone(self);
        tokio::task::spawn_blocking(move || {
            if let Err(err) = me.run_sync() {
                eprintln!("warm runtime caches: {err}");
            }
            me.running.store(false, Ordering::Release);
        });
    }

    pub async fn warm_blocking(self: &Arc<Self>) -> Result<(), String> {
        if self.running.swap(true, Ordering::AcqRel) {
            return Err("runtime cache warmup is already in progress".into());
        }
        let me = Arc::clone(self);
        let res = tokio::task::spawn_blocking(move || me.run_sync())
            .await
            .map_err(|e| e.to_string())?;
        self.running.store(false, Ordering::Release);
        res
    }

    fn run_sync(&self) -> Result<(), String> {
        if self.plan.preload_selectors {
            let started = Instant::now();
            let bytes = self.state.db.warm_selectors();
            eprintln!(
                "warmed {:.1} MiB of compact LPM selectors in {}ms",
                bytes as f64 / (1u64 << 20) as f64,
                started.elapsed().as_millis()
            );
        }
        if self.plan.warm_dataset_page_cache && !self.dataset_warmed.load(Ordering::Acquire) {
            let gb = self.plan.database_bytes as f64 / GIBIBYTE as f64;
            eprintln!("warming {gb:.1} GiB mmap dataset through the kernel page cache");
            let warmed = self.state.db.warm_dataset().map_err(|e| e.to_string())?;
            self.dataset_warmed.store(true, Ordering::Release);
            eprintln!(
                "warmed {:.1} GiB mmap dataset through the kernel page cache",
                warmed as f64 / GIBIBYTE as f64
            );
        }
        Ok(())
    }
}

pub fn spawn_signal_loop(
    state: Arc<ApiState>,
    warmer: Arc<RuntimeWarmer>,
    shutdown_tx: watch::Sender<bool>,
    runtime_cache_control: bool,
) {
    tokio::spawn(async move {
        let (Ok(mut sigint), Ok(mut sigterm), Ok(mut usr1), Ok(mut usr2)) = (
            signal(SignalKind::interrupt()),
            signal(SignalKind::terminate()),
            signal(SignalKind::user_defined1()),
            signal(SignalKind::user_defined2()),
        ) else {
            return;
        };
        loop {
            tokio::select! {
                _ = sigint.recv() => { let _ = shutdown_tx.send(true); break; }
                _ = sigterm.recv() => { let _ = shutdown_tx.send(true); break; }
                _ = usr1.recv() => {
                    state.caches.enable();
                    warmer.spawn_start();
                }
                _ = usr2.recv() => {
                    if runtime_cache_control {
                        let rel = state.caches.disable_and_clear();
                        eprintln!(
                            "released runtime caches: compact_entries={} compact={:.1} MiB resource_entries={} resource={:.1} MiB",
                            rel.compact_entries,
                            rel.compact_bytes as f64 / (1u64 << 20) as f64,
                            rel.resource_entries,
                            rel.resource_bytes as f64 / (1u64 << 20) as f64,
                        );
                    } else {
                        eprintln!("ignoring SIGUSR2 because BGP_API_RUNTIME_CACHE_CONTROL is disabled");
                    }
                }
            }
        }
    });
}
