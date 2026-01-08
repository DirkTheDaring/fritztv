use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use tokio::sync::{Mutex, RwLock};
use tracing::info;

use crate::transcoder::TuningMode;

fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn stable_hash_u64(value: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

use notify::{RecommendedWatcher, RecursiveMode, Watcher, Event};
use tokio::sync::Notify;

#[derive(Clone)]
pub struct HlsManager {
    inner: Arc<Inner>,
}

struct Inner {
    streams: Mutex<HashMap<String, HlsStream>>,
    base_dir: PathBuf,
    _watcher: Mutex<RecommendedWatcher>,
}

struct HlsStream {
    dir: PathBuf,
    last_access: Arc<AtomicU64>,
    playlist_ready: Arc<RwLock<bool>>,
    playlist_ready_notify: Arc<Notify>,
}

async fn clean_hls_dir(dir: &Path) {
    let mut entries = match tokio::fs::read_dir(dir).await {
        Ok(d) => d,
        Err(_) => return,
    };

    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
            if name == "index.m3u8" || (name.starts_with("seg_") && name.ends_with(".ts")) {
                let _ = tokio::fs::remove_file(path).await;
            }
        }
    }
}

impl HlsManager {
    pub fn new(mode: TuningMode, transport: String) -> Self {
        let _ = mode;
        let _ = transport;
        
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        
        // Create the watcher that sends events to our channel.
        let mut watcher = notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
            if let Ok(event) = res {
                let _ = tx.send(event);
            }
        }).expect("Failed to create watcher");

        let base_dir = PathBuf::from("/tmp/fritztv-hls");
        
        // Ensure base dir exists so we can watch it (recursively).
        std::fs::create_dir_all(&base_dir).expect("Failed to create base HLS dir");
        watcher.watch(&base_dir, RecursiveMode::Recursive).expect("Failed to watch HLS dir");

        let inner = Arc::new(Inner {
            streams: Mutex::new(HashMap::new()),
            base_dir,
            _watcher: Mutex::new(watcher),
        });

        // Spawn the event handler loop
        let inner_for_task = Arc::downgrade(&inner);
        tokio::spawn(async move {
            while let Some(event) = rx.recv().await {
                // We only care about creation or modification of "index.m3u8"
                if let Some(path) = event.paths.first() {
                    if let Some(filename) = path.file_name().and_then(|s| s.to_str()) {
                        if filename == "index.m3u8" {
                            // Find which stream this belongs to
                            if let Some(inner) = inner_for_task.upgrade() {
                                let streams = inner.streams.lock().await;
                                // Simple linear scan is fine for O(N) where N is small (num streams)
                                for stream in streams.values() {
                                    if path.starts_with(&stream.dir) {
                                        let mut w = stream.playlist_ready.write().await;
                                        if !*w {
                                            *w = true;
                                            stream.playlist_ready_notify.notify_waiters();
                                        }
                                        break;
                                    }
                                }
                            } else {
                                break; // Inner dropped, exit task
                            }
                        }
                    }
                }
            }
        });

        Self { inner }
    }

    pub async fn get_or_start(&self, id: String, url: String) -> anyhow::Result<PathBuf> {
        let mut streams = self.inner.streams.lock().await;
        if let Some(existing) = streams.get(&id) {
            existing.last_access.store(now_epoch_secs(), Ordering::Relaxed);
            return Ok(existing.dir.clone());
        }

        let hash = stable_hash_u64(&url);
        let dir = self.inner.base_dir.join(format!("{hash:016x}"));
        tokio::fs::create_dir_all(&dir).await?;

        // Ensure stale files from previous runs don't break ffmpeg
        clean_hls_dir(&dir).await;
        let last_access = Arc::new(AtomicU64::new(now_epoch_secs()));
        let playlist_ready = Arc::new(RwLock::new(false));
        let playlist_ready_notify = Arc::new(Notify::new());

        // Note: The watcher is already watching base_dir recursively, so it sees this new dir.

        streams.insert(
            id,
            HlsStream {
                dir: dir.clone(),
                last_access,
                playlist_ready,
                playlist_ready_notify,
            },
        );

        Ok(dir)
    }

    pub async fn touch(&self, id: &str) {
        if let Some(stream) = self.inner.streams.lock().await.get(id) {
            stream.last_access.store(now_epoch_secs(), Ordering::Relaxed);
        }
    }

    pub async fn wait_for_playlist(&self, id: &str, timeout: Duration) -> bool {
        // First check fast path
        let notify = {
            let streams = self.inner.streams.lock().await;
            if let Some(stream) = streams.get(id) {
                if *stream.playlist_ready.read().await {
                    return true;
                }
                stream.playlist_ready_notify.clone()
            } else {
                return false;
            }
        };

        // Wait for notification or timeout
        // Note: There is a race here where the file is created between the check and the notify wait.
        // However, notify_waiters() wakes up everyone. But if we missed the notification, we might wait forever.
        // Safe Pattern: Check -> Notify::notified() (which is cancellation safe) -> Check again.
        // Actually, Notify::notified() only wakes if notification happens *after* await starts? 
        // No, Notify is not edge-triggered loop; it's a semaphore-like one-shot usually?
        // tokio::sync::Notify: "Tasks will be notified if they are awaiting... or if they await *after* notify is called (if permit is stored? No)."
        // "If `notify_waiters()` is called, all waiting tasks are woken... It does NOT store a permit."
        
        // So we need a loop with timeout.
        
        let start = std::time::Instant::now();
        loop {
            if let Ok(_) = tokio::time::timeout(Duration::from_millis(500), notify.notified()).await {
                 // Woken up
                 return true;
            }
            
            // Timeout or spuriously check
            let streams = self.inner.streams.lock().await;
            if let Some(stream) = streams.get(id) {
                if *stream.playlist_ready.read().await {
                    return true;
                }
            }
            
            if start.elapsed() >= timeout {
                return false;
            }
        }
    }

    pub fn playlist_path(dir: &Path) -> PathBuf {
        dir.join("index.m3u8")
    }

    pub fn segment_path(dir: &Path, name: &str) -> Option<PathBuf> {
        // Basic path safety: only allow seg_*.ts
        if !name.starts_with("seg_") || !name.ends_with(".ts") || name.contains('/') || name.contains("..") {
            return None;
        }
        Some(dir.join(name))
    }

    pub async fn prepare_new_session(&self, id: &str) {
        let streams = self.inner.streams.lock().await;
        if let Some(stream) = streams.get(id) {
            info!("HLS new session for {}: cleaning up old segments", id);
            
            // Mark playlist not ready
            let mut w = stream.playlist_ready.write().await;
            *w = false;
            drop(w);

            // Clean dirt
            let dir = stream.dir.clone();
            clean_hls_dir(&dir).await;
            
            // No need to spawn a specific watcher; the global watcher will trigger when the new file appears.
        }
    }
}
