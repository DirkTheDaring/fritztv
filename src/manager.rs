use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{RwLock, broadcast};
use bytes::Bytes;
use crate::transcoder::{Transcoder, TuningMode};
use tracing::info;
use anyhow::anyhow;
use std::time::Duration;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::atomic::AtomicU64;
use std::time::{SystemTime, UNIX_EPOCH};
use std::path::PathBuf;

fn now_epoch_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn query_param<'a>(url: &'a str, key: &str) -> Option<&'a str> {
    let q = url.split_once('?')?.1;
    for part in q.split('&') {
        let (k, v) = part.split_once('=')?;
        if k == key {
            return Some(v);
        }
    }
    None
}

fn mux_key_from_rtsp_url(url: &str) -> String {
    // FritzBox DVB-C SAT>IP URLs embed tuning parameters in the query string.
    // Multiple programs on the same mux differ only in `pids` and can share the same tuner.
    // We also *exclude* `avm` here because we will assign a tuner slot ourselves.
    let keys = ["freq", "bw", "msys", "mtype", "sr", "specinv"];
    let mut out = String::new();
    for k in keys {
        if !out.is_empty() {
            out.push('&');
        }
        out.push_str(k);
        out.push('=');
        if let Some(v) = query_param(url, k) {
            out.push_str(v);
        }
    }
    out
}

fn avm_from_rtsp_url(url: &str) -> Option<u32> {
    query_param(url, "avm").and_then(|v| v.parse::<u32>().ok())
}

fn set_query_param(url: &str, key: &str, value: &str) -> String {
    let Some((base, query)) = url.split_once('?') else {
        return format!("{url}?{key}={value}");
    };

    let mut pairs: Vec<(String, String)> = Vec::new();
    let mut found = false;
    for part in query.split('&') {
        if part.is_empty() {
            continue;
        }
        let (k, v) = match part.split_once('=') {
            Some((k, v)) => (k, v),
            None => (part, ""),
        };
        if k == key {
            pairs.push((k.to_string(), value.to_string()));
            found = true;
        } else {
            pairs.push((k.to_string(), v.to_string()));
        }
    }
    if !found {
        pairs.push((key.to_string(), value.to_string()));
    }

    let rebuilt = pairs
        .into_iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("&");

    format!("{base}?{rebuilt}")
}

fn is_stream_active(stream: &ActiveStream, now: u64, idle_grace_seconds: u64) -> bool {
    let count = stream.client_count.load(Ordering::Acquire);
    let hls_last = stream.hls_last_access.load(Ordering::Relaxed);
    let hls_active = hls_last != 0 && now.saturating_sub(hls_last) <= idle_grace_seconds;
    count > 0 || hls_active
}

pub struct ActiveStream {
    pub tx: broadcast::Sender<Bytes>,
    pub header: Arc<RwLock<Option<Bytes>>>,
    pub cache: Arc<RwLock<std::collections::VecDeque<Bytes>>>,
    pub client_count: Arc<AtomicUsize>,
    pub hls_last_access: Arc<AtomicU64>,
    pub mux_key: String,
    pub avm: u32,
    pub effective_url: String,
    _transcoder: Transcoder,
}

#[derive(Clone)]
pub struct ClientGuard {
    id: String,
    client_count: Arc<AtomicUsize>,
}

impl Drop for ClientGuard {
    fn drop(&mut self) {
        // Saturating decrement; avoid underflow if Drop runs unexpectedly.
        let prev = match self.client_count.fetch_update(
            Ordering::AcqRel,
            Ordering::Acquire,
            |v| v.checked_sub(1),
        ) {
            Ok(prev) => prev,
            Err(current) => current,
        };
        let new = prev.saturating_sub(1);
        info!("Client disconnected from {} (client_count={})", self.id, new);
    }
}

#[derive(Clone)]
pub struct StreamManager {
    streams: Arc<RwLock<HashMap<String, Arc<ActiveStream>>>>,
    mode: TuningMode,
    transport: String,
    max_parallel_streams: usize,
}

impl StreamManager {
    pub fn new(mode: TuningMode, transport: String, max_parallel_streams: usize) -> Self {
        Self {
            streams: Arc::new(RwLock::new(HashMap::new())),
            mode,
            transport,
            max_parallel_streams: max_parallel_streams.max(1),
        }
    }

    // Returns receiver, header store, and cache snapshot
    pub async fn get_or_start_stream(
        &self,
        id: String,
        url: String,
        hls_dir: Option<PathBuf>,
    ) -> anyhow::Result<(
        broadcast::Receiver<Bytes>,
        Arc<RwLock<Option<Bytes>>>,
        Vec<Bytes>,
        ClientGuard,
    )> {
        let mut streams = self.streams.write().await;

        if let Some(stream) = streams.get(&id) {
            let new_count = stream.client_count.fetch_add(1, Ordering::AcqRel).saturating_add(1);
            info!("Client connected to {} (client_count={})", id, new_count);
            let cache_snapshot = {
                let c = stream.cache.read().await;
                // Find the first 'moof' atom to ensure we start at a fragment boundary/keyframe
                let start_idx = c.iter().position(|chunk| {
                    chunk.len() >= 8 && &chunk[4..8] == b"moof"
                }).unwrap_or(c.len());

                c.iter().skip(start_idx).cloned().collect()
            };
            let guard = ClientGuard {
                id: id.clone(),
                client_count: stream.client_count.clone(),
            };
            return Ok((stream.tx.subscribe(), stream.header.clone(), cache_snapshot, guard));
        }

        // Allocate a tuner slot (avm) instead of rejecting "tuning conflicts".
        // - If another *active* stream is on the same mux, reuse its avm.
        // - Otherwise, pick a free avm in 1..=max_parallel_streams.
        let now = now_epoch_secs();
        let idle_grace_seconds: u64 = 60;
        let new_mux = mux_key_from_rtsp_url(&url);
        let mut chosen_avm: Option<u32> = None;

        for stream in streams.values() {
            if !is_stream_active(stream, now, idle_grace_seconds) {
                continue;
            }
            if stream.mux_key == new_mux {
                chosen_avm = Some(stream.avm);
                break;
            }
        }

        if chosen_avm.is_none() {
            let mut used = std::collections::HashSet::<u32>::new();
            for stream in streams.values() {
                if is_stream_active(stream, now, idle_grace_seconds) {
                    used.insert(stream.avm);
                }
            }
            for avm in 1..=(self.max_parallel_streams as u32) {
                if !used.contains(&avm) {
                    chosen_avm = Some(avm);
                    break;
                }
            }
        }

        let chosen_avm = chosen_avm.or_else(|| avm_from_rtsp_url(&url)).unwrap_or(1);
        let effective_url = set_query_param(&url, "avm", &chosen_avm.to_string());

        // Note: keep the existing stream-count guard as a coarse safety cap.
        // The FritzBox tuner limit is modeled by avm allocation above.

        if streams.len() >= self.max_parallel_streams {
            return Err(anyhow!(
                "max parallel streams reached ({})",
                self.max_parallel_streams
            ));
        }

        info!(
            "Starting new stream for {} (mux={} avm={} effective_url={})",
            id,
            new_mux,
            chosen_avm,
            effective_url
        );
        let (tx, rx) = broadcast::channel(8192);
        let header = Arc::new(RwLock::new(None));
        let cache = Arc::new(RwLock::new(std::collections::VecDeque::new()));
        let client_count = Arc::new(AtomicUsize::new(1));
        info!("Client connected to {} (client_count=1)", id);
        
        let hls_last_access = Arc::new(AtomicU64::new(if hls_dir.is_some() { now_epoch_secs() } else { 0 }));
        let transcoder = Transcoder::new(
            effective_url.clone(),
            tx.clone(),
            header.clone(),
            self.mode,
            self.transport.clone(),
            hls_dir,
        );
        
        let active_stream = Arc::new(ActiveStream {
            tx: tx.clone(),
            header: header.clone(),
            cache: cache.clone(),
            client_count: client_count.clone(),
            hls_last_access: hls_last_access.clone(),
            mux_key: new_mux,
            avm: chosen_avm,
            effective_url,
            _transcoder: transcoder,
        });

        streams.insert(id.clone(), active_stream);

        // Spawn cleanup task
        let streams_clone = self.streams.clone();
        let id_clone = id.clone();
        let client_count_clone = client_count.clone();
        let hls_last_access_clone = hls_last_access.clone();
        
        // Spawn cache maintainer
        let mut cache_rx = tx.clone().subscribe();
        let cache_access = cache.clone();
        tokio::spawn(async move {
            let max_cache_size = 8 * 1024 * 1024; // 8MB
            let mut current_size = 0;
            loop {
                match cache_rx.recv().await {
                    Ok(chunk) => {
                        let mut c = cache_access.write().await;
                        let chunk_len = chunk.len();
                        c.push_back(chunk);
                        current_size += chunk_len;
                        while current_size > max_cache_size {
                            if let Some(removed) = c.pop_front() {
                                current_size -= removed.len();
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        // Cache receiver fell behind. Skip missed items and keep caching new ones.
                        continue;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        break;
                    }
                }
            }
        });

        tokio::spawn(async move {
            let mut idle_seconds: u32 = 0;
            // Many players briefly disconnect/reconnect (range probing, reloads, etc.).
            // Don’t tear down the transcoder on a short-lived 0-listener window.
            // Real-world players sometimes download in bursts; keep the stream alive longer
            // than a few seconds even if client_count temporarily hits 0.
            let idle_grace_seconds: u32 = 60;
            loop {
                tokio::time::sleep(Duration::from_millis(1000)).await;
                let count = client_count_clone.load(Ordering::Acquire);
                let hls_last = hls_last_access_clone.load(Ordering::Relaxed);
                let hls_active = hls_last != 0 && now_epoch_secs().saturating_sub(hls_last) <= idle_grace_seconds as u64;

                if count == 0 && !hls_active {
                    idle_seconds = idle_seconds.saturating_add(1);
                    if idle_seconds >= idle_grace_seconds {
                        info!(
                            "Stream {} has no listeners for {}s, cleaning up",
                            id_clone,
                            idle_grace_seconds
                        );
                        let mut streams = streams_clone.write().await;
                        streams.remove(&id_clone);
                        break;
                    }
                } else {
                    idle_seconds = 0;
                }
            }
        });

        let guard = ClientGuard { id: id.clone(), client_count };
        Ok((rx, header, Vec::new(), guard))
    }

    pub async fn ensure_stream(
        &self,
        id: String,
        url: String,
        hls_dir: Option<PathBuf>,
    ) -> anyhow::Result<()> {
        let mut streams = self.streams.write().await;
        if streams.contains_key(&id) {
            return Ok(());
        }

        // Same tuner-slot allocation as get_or_start_stream.
        let now = now_epoch_secs();
        let idle_grace_seconds: u64 = 60;
        let new_mux = mux_key_from_rtsp_url(&url);
        let mut chosen_avm: Option<u32> = None;

        for stream in streams.values() {
            if !is_stream_active(stream, now, idle_grace_seconds) {
                continue;
            }
            if stream.mux_key == new_mux {
                chosen_avm = Some(stream.avm);
                break;
            }
        }

        if chosen_avm.is_none() {
            let mut used = std::collections::HashSet::<u32>::new();
            for stream in streams.values() {
                if is_stream_active(stream, now, idle_grace_seconds) {
                    used.insert(stream.avm);
                }
            }
            for avm in 1..=(self.max_parallel_streams as u32) {
                if !used.contains(&avm) {
                    chosen_avm = Some(avm);
                    break;
                }
            }
        }

        let chosen_avm = chosen_avm.or_else(|| avm_from_rtsp_url(&url)).unwrap_or(1);
        let effective_url = set_query_param(&url, "avm", &chosen_avm.to_string());

        if streams.len() >= self.max_parallel_streams {
            return Err(anyhow!(
                "max parallel streams reached ({})",
                self.max_parallel_streams
            ));
        }

        info!(
            "Starting new stream for {} (hls-only, mux={} avm={} effective_url={})",
            id,
            new_mux,
            chosen_avm,
            effective_url
        );
        let (tx, _rx) = broadcast::channel(8192);
        let header = Arc::new(RwLock::new(None));
        let cache = Arc::new(RwLock::new(std::collections::VecDeque::new()));
        let client_count = Arc::new(AtomicUsize::new(0));
        let hls_last_access = Arc::new(AtomicU64::new(if hls_dir.is_some() { now_epoch_secs() } else { 0 }));

        let transcoder = Transcoder::new(
            effective_url.clone(),
            tx.clone(),
            header.clone(),
            self.mode,
            self.transport.clone(),
            hls_dir,
        );

        let active_stream = Arc::new(ActiveStream {
            tx: tx.clone(),
            header: header.clone(),
            cache: cache.clone(),
            client_count: client_count.clone(),
            hls_last_access: hls_last_access.clone(),
            mux_key: new_mux,
            avm: chosen_avm,
            effective_url,
            _transcoder: transcoder,
        });

        streams.insert(id.clone(), active_stream);

        // Spawn cache maintainer
        let mut cache_rx = tx.clone().subscribe();
        let cache_access = cache.clone();
        tokio::spawn(async move {
            let max_cache_size = 8 * 1024 * 1024; // 8MB
            let mut current_size = 0;
            loop {
                match cache_rx.recv().await {
                    Ok(chunk) => {
                        let mut c = cache_access.write().await;
                        let chunk_len = chunk.len();
                        c.push_back(chunk);
                        current_size += chunk_len;
                        while current_size > max_cache_size {
                            if let Some(removed) = c.pop_front() {
                                current_size -= removed.len();
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        });

        // Spawn cleanup task
        let streams_clone = self.streams.clone();
        let id_clone = id.clone();
        let client_count_clone = client_count.clone();
        let hls_last_access_clone = hls_last_access.clone();
        tokio::spawn(async move {
            let mut idle_seconds: u32 = 0;
            let idle_grace_seconds: u32 = 60;
            loop {
                tokio::time::sleep(Duration::from_millis(1000)).await;
                let count = client_count_clone.load(Ordering::Acquire);
                let hls_last = hls_last_access_clone.load(Ordering::Relaxed);
                let hls_active = hls_last != 0 && now_epoch_secs().saturating_sub(hls_last) <= idle_grace_seconds as u64;

                if count == 0 && !hls_active {
                    idle_seconds = idle_seconds.saturating_add(1);
                    if idle_seconds >= idle_grace_seconds {
                        info!(
                            "Stream {} has no listeners for {}s, cleaning up",
                            id_clone,
                            idle_grace_seconds
                        );
                        let mut streams = streams_clone.write().await;
                        streams.remove(&id_clone);
                        break;
                    }
                } else {
                    idle_seconds = 0;
                }
            }
        });

        Ok(())
    }

    pub async fn touch_hls(&self, id: &str) {
        if let Some(stream) = self.streams.read().await.get(id) {
            stream.hls_last_access.store(now_epoch_secs(), Ordering::Relaxed);
        }
    }
}
