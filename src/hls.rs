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

#[derive(Clone)]
pub struct HlsManager {
    inner: Arc<Inner>,
}

struct Inner {
    streams: Mutex<HashMap<String, HlsStream>>,
    base_dir: PathBuf,
}

struct HlsStream {
    dir: PathBuf,
    last_access: Arc<AtomicU64>,
    playlist_ready: Arc<RwLock<bool>>,
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
        Self {
            inner: Arc::new(Inner {
                streams: Mutex::new(HashMap::new()),
                base_dir: PathBuf::from("/tmp/fritztv-hls"),
            }),
        }
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

        // Ensure stale files from previous runs don't break ffmpeg (it will prompt
        // for overwrite and then exit if stdin isn't available).
        clean_hls_dir(&dir).await;
        let last_access = Arc::new(AtomicU64::new(now_epoch_secs()));
        let playlist_ready = Arc::new(RwLock::new(false));

        // NOTE: We intentionally do NOT "forget" HLS streams on an idle timer.
        // A stream may be actively transcoding MP4 while HLS clients are absent.
        // Forgetting would cause the next HLS request to re-create the entry and
        // (critically) re-run clean_hls_dir(), deleting the playlist/segments while
        // ffmpeg is still writing them, which can trigger player buffering.

        // Mark playlist_ready once index exists (created by the main Transcoder).
        let dir_for_ready = dir.clone();
        let ready_flag = Arc::clone(&playlist_ready);
        tokio::spawn(async move {
            let playlist_path = dir_for_ready.join("index.m3u8");
            for _ in 0..200 {
                if tokio::fs::metadata(&playlist_path).await.is_ok() {
                    let mut w = ready_flag.write().await;
                    *w = true;
                    return;
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
        });

        streams.insert(
            id,
            HlsStream {
                dir: dir.clone(),
                last_access,
                playlist_ready,
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
        let start = now_epoch_secs();
        loop {
            if let Some(stream) = self.inner.streams.lock().await.get(id) {
                if *stream.playlist_ready.read().await {
                    return true;
                }
            }
            if now_epoch_secs().saturating_sub(start) >= timeout.as_secs() {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
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
}
