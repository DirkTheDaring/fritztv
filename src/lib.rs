pub mod channels;
pub mod hls;
pub mod manager;
pub mod transcoder;

use axum::{
    extract::{Path, State},
    response::{Html, IntoResponse, Json},
    routing::{get, post},
    Router,
};
use axum::body::Body;
use axum::http::Method;
use axum::http::Uri;
use channels::Channel;
use hls::HlsManager;
use manager::StreamManager;
use std::sync::Arc;
use futures::StreamExt;
use futures::stream::Stream;
use std::pin::Pin;
use std::task::{Context, Poll};
use axum::http::HeaderMap;
use serde::Deserialize;
use tracing::{info, warn};

struct AppState {
    channels: Vec<Channel>,
    stream_manager: StreamManager,
    hls_manager: HlsManager,
}

use crate::transcoder::TuningMode;

struct GuardedStream {
    _guard: manager::ClientGuard,
    inner: Pin<Box<dyn Stream<Item = Result<bytes::Bytes, std::io::Error>> + Send>>,
    id: usize,
    last_log_time: std::time::Instant,
    bytes_since_last_log: usize,
}

impl Stream for GuardedStream {
    type Item = Result<bytes::Bytes, std::io::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let res = self.inner.as_mut().poll_next(cx);
        if let Poll::Ready(Some(Ok(ref bytes))) = res {
            self.bytes_since_last_log += bytes.len();
            let elapsed = self.last_log_time.elapsed();
            if elapsed >= std::time::Duration::from_secs(5) {
                let bytes = self.bytes_since_last_log;
                let secs = elapsed.as_secs_f64();
                let rate_kb = (bytes as f64 / secs) / 1024.0;
                info!("Stream bandwidth: channel_id={} rate={:.2} KB/s", self.id, rate_kb);
                self.last_log_time = std::time::Instant::now();
                self.bytes_since_last_log = 0;
            }
        }
        res
    }
}

pub fn create_app(
    channels: Vec<Channel>,
    mode: TuningMode,
    transport: String,
    max_parallel_streams: usize,
    idle_timeout: u64,
) -> Router {
    let stream_transport = transport.clone();
    let state = Arc::new(AppState {
        channels,
        stream_manager: StreamManager::new(mode, stream_transport, max_parallel_streams, idle_timeout),
        hls_manager: HlsManager::new(mode, transport),
    });

    Router::new()
        .route("/", get(index_handler))
        .route("/api/channels", get(channels_api_handler))
        .route("/api/client-log", post(client_log_handler))
        .route("/stream/{id}", get(stream_handler))
        .route(
            "/hls/{id}/index.m3u8",
            get(hls_playlist_handler).head(hls_playlist_handler),
        )
        .route(
            "/hls/{id}/{segment}",
            get(hls_segment_handler).head(hls_segment_handler),
        )
        .route("/watch/{id}", get(watch_handler))
        .fallback(fallback_handler)
        .with_state(state)
}

async fn fallback_handler(method: Method, uri: Uri, headers: HeaderMap) -> impl IntoResponse {
    let user_agent = headers
        .get(axum::http::header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("<none>");
    info!(
        "HTTP 404: method={} uri={} UA=\"{}\"",
        method,
        uri,
        user_agent
    );
    axum::response::Response::builder()
        .status(404)
        .body(Body::from("Not found"))
        .unwrap()
}

#[derive(Deserialize)]
struct ClientLogEvent {
    id: usize,
    event: String,
    detail: Option<String>,
}

async fn client_log_handler(
    headers: HeaderMap,
    Json(payload): Json<ClientLogEvent>,
) -> impl IntoResponse {
    let user_agent = headers
        .get(axum::http::header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("<none>");
    info!(
        "CLIENT event: id={} event={} detail={:?} UA=\"{}\"",
        payload.id,
        payload.event,
        payload.detail,
        user_agent
    );
    axum::response::Response::builder()
        .status(204)
        .body(Body::empty())
        .unwrap()
}

pub async fn fetch_channels(url: &str) -> anyhow::Result<Vec<Channel>> {
    let client = reqwest::Client::builder()
        .danger_accept_invalid_certs(true)
        .build()?;
    let resp = client.get(url)
        .send().await?;
    let text = resp.text().await?;
    channels::parse_m3u(&text)
}

async fn index_handler(State(state): State<Arc<AppState>>) -> Html<String> {
    let mut html = String::from(r#"
    <!DOCTYPE html>
    <html lang="en">
    <head>
        <meta charset="UTF-8">
        <meta name="viewport" content="width=device-width, initial-scale=1.0, maximum-scale=1.0, user-scalable=no">
        <meta name="apple-mobile-web-app-capable" content="yes">
        <meta name="apple-mobile-web-app-status-bar-style" content="black-translucent">
        <title>Fritztv Channels</title>
        <style>
            :root {
                --bg-color: #0d0d0d;
                --card-bg: #1a1a1a;
                --text-main: #ffffff;
                --text-muted: #a0a0a0;
                --accent-color: #e50914; /* Netflix-like red accent or just clear blue */
                --hover-scale: 1.03;
            }
            body { 
                font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif;
                margin: 0; padding: 20px; 
                background: var(--bg-color); color: var(--text-main);
                -webkit-font-smoothing: antialiased;
            }
            header {
                padding: 10px 0 30px;
                text-align: center;
            }
            h1 { font-size: 2rem; margin: 0; font-weight: 700; letter-spacing: -0.5px; }
            .grid { 
                display: grid; 
                grid-template-columns: repeat(auto-fill, minmax(160px, 1fr)); 
                gap: 16px; 
                max-width: 1200px; margin: 0 auto;
            }
            .card { 
                background: var(--card-bg); 
                padding: 20px; 
                border-radius: 12px; 
                text-decoration: none; 
                color: var(--text-main); 
                transition: all 0.2s cubic-bezier(0.25, 0.46, 0.45, 0.94);
                display: flex; flex-direction: column; align-items: center; justify-content: center;
                text-align: center;
                aspect-ratio: 1.5; /* Rectangular cards */
                position: relative;
                overflow: hidden;
                border: 1px solid rgba(255,255,255,0.05);
            }
            .card:hover { 
                transform: scale(var(--hover-scale)); 
                background: #252525;
                box-shadow: 0 10px 20px rgba(0,0,0,0.5);
                border-color: rgba(255,255,255,0.2);
            }
            .card:active { transform: scale(0.98); }
            .card-name { font-weight: 600; font-size: 1.1rem; }
            .card-icon { 
                font-size: 2rem; margin-bottom: 10px; opacity: 0.7; 
            }
            @media (max-width: 600px) {
                .grid { grid-template-columns: repeat(2, 1fr); gap: 10px; }
                body { padding: 15px; }
            }
        </style>
    </head>
    <body>
        <header>
            <h1>Fritztv</h1>
        </header>
        <div class="grid">
    "#);

    for (i, channel) in state.channels.iter().enumerate() {
        // Generate a pseudo-random color/icon based on name hash? Or just generic TV icon
        html.push_str(&format!(
            r#"<a href="/watch/{}" class="card">
                <div class="card-icon">📺</div>
                <div class="card-name">{}</div>
            </a>"#,
            i, channel.name
        ));
    }

    html.push_str(r#"
        </div>
    </body>
    </html>
    "#);

    Html(html)
}

async fn watch_handler(
    Path(id): Path<usize>,
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if id >= state.channels.len() {
        return axum::response::Response::builder()
            .status(404)
            .body(Body::from("Channel not found"))
            .unwrap();
    }

    let channel = &state.channels[id];

    let user_agent = headers
        .get(axum::http::header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("<none>");
    info!("HTTP watch request: id={} UA=\"{}\"", id, user_agent);

    let html = format!(r#"
    <!DOCTYPE html>
    <html lang="en">
    <head>
        <meta charset="UTF-8">
        <meta name="viewport" content="width=device-width, initial-scale=1.0, maximum-scale=1.0, user-scalable=yes">
        <meta name="apple-mobile-web-app-capable" content="yes">
        <meta name="apple-mobile-web-app-status-bar-style" content="black">
        <title>Watching {}</title>
        <style>
            :root {{
                --bg-color: #000000;
                --text-color: #ffffff;
                --accent-color: #3b82f6;
            }}
            body {{ 
                margin: 0; padding: 0; 
                background: var(--bg-color); color: var(--text-color);
                font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
                height: 100vh; display: flex; flex-direction: column;
            }}
            .header {{
                padding: 15px 20px;
                background: rgba(0,0,0,0.8);
                backdrop-filter: blur(10px);
                position: absolute; top: 0; left: 0; right: 0;
                z-index: 20;
                display: flex; align-items: center; justify-content: space-between;
                transition: opacity 0.3s;
            }}
            .back-link {{ 
                color: #ddd; text-decoration: none; font-size: 1rem; 
                display: flex; align-items: center; gap: 5px;
                padding: 8px 12px; background: rgba(255,255,255,0.1); border-radius: 20px;
                font-weight: 500;
            }}
            .back-link:hover {{ background: rgba(255,255,255,0.2); color: white; }}
            .channel-title {{ font-size: 1.1rem; font-weight: 600; opacity: 0.9; }}
            
            .video-wrapper {{ 
                flex: 1; 
                display: flex; align-items: center; justify-content: center; 
                position: relative;
                width: 100%; height: 100%;
            }}
            
            video {{ 
                width: 100%; max-height: 100%; 
                outline: none;
            }}

            .loader-overlay {{
                position: absolute; top: 0; left: 0; right: 0; bottom: 0;
                background: rgba(0,0,0,0.8);
                display: flex; flex-direction: column; align-items: center; justify-content: center;
                z-index: 10;
            }}
            .spinner {{
                width: 50px; height: 50px;
                border: 4px solid rgba(255,255,255,0.1);
                border-radius: 50%;
                border-top-color: #fff;
                animation: spin 1s ease-in-out infinite;
                margin-bottom: 20px;
            }}
            .loader-text {{ color: #aaa; font-size: 0.9rem; letter-spacing: 0.5px; text-transform: uppercase; }}
            
            @keyframes spin {{ to {{ transform: rotate(360deg); }} }}
            .hidden {{ opacity: 0; pointer-events: none; transition: opacity 0.5s; }}
            
            /* Controls idle hide */
            body.idle .header {{ opacity: 0; pointer-events: none; }}
        </style>
    </head>
    <body>
        <div class="header" id="controls">
            <a href="/" class="back-link">
                <svg width="20" height="20" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><polyline points="15 18 9 12 15 6"></polyline></svg>
                Channels
            </a>
            <div class="channel-title">{}</div>
            <div style="width: 80px;"></div> <!-- Spacer for balance -->
        </div>

        <div class="video-wrapper">
            <div id="loader" class="loader-overlay">
                <div class="spinner"></div>
                <div class="loader-text">Connecting Stream...</div>
            </div>
            <video id="player" playsinline controls autoplay preload="auto"></video>
        </div>

        <script>
            const player = document.getElementById('player');
            const loader = document.getElementById('loader');
            let idleTimer;
            let overlayPinned = false;

            const channelId = {};

            const isIOS = (() => {{
                const ua = navigator.userAgent || '';
                // iPadOS 13+ reports as MacIntel but has touch points.
                return /iPad|iPhone|iPod/.test(ua) || (navigator.platform === 'MacIntel' && navigator.maxTouchPoints > 1);
            }})();

            const isSafari = (() => {{
                const ua = navigator.userAgent || '';
                // Safari on macOS/iOS, but not Chrome/Edge/Firefox/Opera.
                return /Safari\//.test(ua) && !/Chrome\//.test(ua) && !/Chromium\//.test(ua) && !/Edg\//.test(ua) && !/Firefox\//.test(ua) && !/OPR\//.test(ua);
            }})();

            const isChrome = (() => {{
                const ua = navigator.userAgent || '';
                // Chrome/Chromium, but not Edge/Opera.
                return (/Chrome\//.test(ua) || /Chromium\//.test(ua)) && !/Edg\//.test(ua) && !/OPR\//.test(ua);
            }})();

            // Keep initial state unmuted. On iOS/iPadOS, unmuted playback must be started
            // by a user gesture (autoplay with sound is blocked by Safari).
            player.muted = false;

            function logClient(event, detail) {{
                try {{
                    fetch('/api/client-log', {{
                        method: 'POST',
                        headers: {{ 'Content-Type': 'application/json' }},
                        body: JSON.stringify({{ id: channelId, event, detail: detail ?? null }})
                    }});
                }} catch (_) {{}}
            }}

            // Explicitly choose a single source.
            // Safari (and iOS Safari) can behave oddly with multiple <source> fallbacks,
            // sometimes fetching the playlist but never committing to segment requests.
            const hlsUrl = "/hls/" + channelId + "/index.m3u8";
            const mp4Url = "/stream/" + channelId;
            // Only Safari/iOS can reliably play HLS natively.
            const enableHls = isIOS || isSafari;

            async function waitForHlsReady(url) {{
                logClient('hls_probe_start', url);
                const deadline = Date.now() + 20000;
                while (Date.now() < deadline) {{
                    try {{
                        const resp = await fetch(url, {{ cache: 'no-store' }});
                        const status = resp.status;
                        const body = await resp.text();
                        if (status === 200 && body.indexOf('seg_') !== -1) {{
                            logClient('hls_probe_ok', 'status=' + status);
                            return true;
                        }}
                        logClient('hls_probe_retry', 'status=' + status);
                    }} catch (e) {{
                        logClient('hls_probe_retry', String(e));
                    }}
                    await new Promise(r => setTimeout(r, 250));
                }}
                logClient('hls_probe_timeout');
                return false;
            }}

            async function selectSource() {{
                if (enableHls) {{
                    // Safari can reject an HLS source if the initial playlist is empty/invalid.
                    // Probe until the playlist contains at least one segment before assigning.
                    const ok = await waitForHlsReady(hlsUrl);
                    if (ok) {{
                        player.src = hlsUrl;
                        logClient('source_selected', 'hls');
                    }} else {{
                        player.src = mp4Url;
                        logClient('source_selected', 'mp4_fallback');
                    }}
                }} else {{
                    player.src = mp4Url;
                    logClient('source_selected', 'mp4');
                }}
                player.load();
            }}

            function hideLoader() {{
                if (overlayPinned) return;
                loader.classList.add('hidden');
            }}
            
            function showLoader(text, pin = false) {{
                overlayPinned = !!pin;
                loader.querySelector('.loader-text').innerText = text;
                loader.classList.remove('hidden');
            }}

            // iOS/iPadOS: require a user gesture to start audio.
            if (isIOS) {{
                showLoader('Tap to play', true);
            }}

            player.addEventListener('canplay', () => hideLoader());
            player.addEventListener('playing', () => hideLoader());
            player.addEventListener('waiting', () => showLoader('Buffering...'));
            player.addEventListener('loadedmetadata', () => hideLoader());

            player.addEventListener('play', () => logClient('play'));
            player.addEventListener('playing', () => logClient('playing'));
            player.addEventListener('waiting', () => logClient('waiting'));
            player.addEventListener('stalled', () => logClient('stalled'));
            player.addEventListener('ended', () => logClient('ended'));

            function snapshotState() {{
                let buffered = '';
                try {{
                    if (player.buffered && player.buffered.length > 0) {{
                        buffered = `${{player.buffered.start(0)}}-${{player.buffered.end(player.buffered.length - 1)}}`;
                    }}
                }} catch (_) {{}}

                const err = player.error ? String(player.error.code) : 'none';
                return 'ns=' + player.networkState +
                    ' rs=' + player.readyState +
                    ' ct=' + player.currentTime +
                    ' muted=' + player.muted +
                    ' paused=' + player.paused +
                    ' ended=' + player.ended +
                    ' buf=' + buffered +
                    ' err=' + err +
                    ' src=' + (player.currentSrc || player.src || '');
            }}

            // Periodic telemetry (helps when Safari never fires 'error')
            let telemetryCount = 0;
            const telemetryTimer = setInterval(() => {{
                telemetryCount += 1;
                logClient('state', snapshotState());
                // Stop only once we have stronger indication playback is really happening.
                // Safari can reach readyState=1 just from fetching the playlist.
                if (player.readyState >= 3 || player.currentTime > 0 || player.error) {{
                    logClient('state_stop', snapshotState());
                    clearInterval(telemetryTimer);
                    return;
                }}
                if (telemetryCount >= 60) {{
                    clearInterval(telemetryTimer);
                }}
            }}, 1000);

            // iOS Safari often blocks autoplay unless muted and/or initiated by user gesture.
            // If autoplay fails, show a tap-to-play prompt.
            async function tryPlay() {{
                async function playWithTimeout() {{
                    const playPromise = player.play();
                    // Some browsers can leave play() pending while deciding.
                    const timeoutPromise = new Promise((_, reject) =>
                        setTimeout(() => reject(new Error('play_timeout')), 5000)
                    );
                    await Promise.race([playPromise, timeoutPromise]);
                }}

                try {{
                    await playWithTimeout();
                    logClient('play_resolved');
                }} catch (e) {{
                    if (String(e) === 'Error: play_timeout') {{
                        logClient('play_pending', snapshotState());
                        return;
                    }}

                    const errStr = String(e);
                    // Chrome: prefer audio-on by default. If unmuted autoplay is blocked,
                    // fall back to muted autoplay so video still starts automatically.
                    // Keep an overlay so the user can enable sound with one tap.
                    if (!isIOS && isChrome && !player.muted && (errStr.indexOf('NotAllowedError') !== -1 || errStr.indexOf('NotAllowed') !== -1)) {{
                        logClient('autoplay_blocked_chrome', errStr);
                        try {{
                            player.muted = true;
                            await playWithTimeout();
                            logClient('play_resolved_muted_chrome');
                            showLoader('Tap for sound', true);
                            return;
                        }} catch (e2) {{
                            logClient('play_rejected', String(e2));
                            showLoader('Tap to play', true);
                            return;
                        }}
                    }}

                    // Other desktop browsers: if unmuted autoplay is blocked, retry once muted.
                    if (!isIOS && !isChrome && !player.muted && (errStr.indexOf('NotAllowedError') !== -1 || errStr.indexOf('NotAllowed') !== -1)) {{
                        logClient('autoplay_muted_fallback', errStr);
                        try {{
                            player.muted = true;
                            await playWithTimeout();
                            logClient('play_resolved_muted');
                            return;
                        }} catch (e2) {{
                            logClient('play_rejected', String(e2));
                            showLoader('Tap to play');
                            return;
                        }}
                    }}

                    logClient('play_rejected', errStr);
                    showLoader('Tap to play');
                }}
            }}

            loader.addEventListener('click', async () => {{
                // User gesture: allow enabling audio.
                await sourceReadyPromise;
                player.muted = false;
                player.volume = 1.0;
                overlayPinned = false;
                if (!player.paused) {{
                    hideLoader();
                    return;
                }}
                await tryPlay();
            }});

            player.addEventListener('error', () => {{
                const code = player.error ? player.error.code : null;
                logClient('error', code !== null ? String(code) : 'unknown');
                showLoader('Playback error');
            }});

            // Start selecting/loading the source immediately.
            const sourceReadyPromise = selectSource();

            // Desktop: attempt autoplay.
            // iOS/iPadOS: wait for user gesture (tap overlay) so audio can start unmuted.
            if (!isIOS) {{
                sourceReadyPromise.then(() => tryPlay());
            }}
            
            // Auto hide controls logic
            function resetIdleTimer() {{
                document.body.classList.remove('idle');
                clearTimeout(idleTimer);
                idleTimer = setTimeout(() => {{
                    if(!player.paused) {{
                        document.body.classList.add('idle');
                    }}
                }}, 3000);
            }}
            
            ['mousemove', 'touchstart', 'click', 'keydown'].forEach(evt => 
                window.addEventListener(evt, resetIdleTimer)
            );
            
            resetIdleTimer();
        </script>
    </body>
    </html>
    "#, channel.name, channel.name, id);

    axum::response::Response::builder()
        .header("Content-Type", "text/html")
        .body(Body::from(html))
        .unwrap()
}

async fn channels_api_handler(State(state): State<Arc<AppState>>) -> Json<Vec<Channel>> {
    Json(state.channels.clone())
}

async fn hls_playlist_handler(
    Path(id): Path<usize>,
    State(state): State<Arc<AppState>>,
    method: Method,
    headers: HeaderMap,
) -> impl IntoResponse {
    if id >= state.channels.len() {
        return axum::response::Response::builder()
            .status(404)
            .body(Body::from("Channel not found"))
            .unwrap();
    }

    let channel = &state.channels[id];
    let stream_id = channel.url.clone();

    let user_agent = headers
        .get(axum::http::header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("<none>");
    let range = headers
        .get(axum::http::header::RANGE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("<none>");
    let accept = headers
        .get(axum::http::header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("<none>");

    info!(
        "HTTP HLS playlist request: id={} method={} UA=\"{}\" Range=\"{}\" Accept=\"{}\"",
        id,
        method,
        user_agent
        ,
        range,
        accept
    );

    let dir = match state
        .hls_manager
        .get_or_start(stream_id.clone(), channel.url.clone())
        .await
    {
        Ok(d) => d,
        Err(e) => {
            return axum::response::Response::builder()
                .status(500)
                .body(Body::from(format!("Failed to start HLS: {e}")))
                .unwrap();
        }
    };

    // Ensure the single shared transcoder is running and is configured to write HLS
    // into this directory (no second RTSP session).
    if let Err(e) = state
        .stream_manager
        .ensure_stream(stream_id.clone(), channel.url.clone(), Some(dir.clone()), Some(&state.hls_manager))
        .await
    {
        warn!("HLS ensure_stream rejected: id={} err={}", id, e);
        return axum::response::Response::builder()
            .status(503)
            .header("Cache-Control", "no-store")
            .body(Body::from(format!("Stream limit reached: {e}")))
            .unwrap();
    }
    state.stream_manager.touch_hls(&stream_id).await;
    state.hls_manager.touch(&stream_id).await;

    let playlist_path = HlsManager::playlist_path(&dir);

    // Safari often probes with HEAD first. Respond quickly so it doesn't decide HLS is unavailable
    // and fall back to the MP4 source.
    if method == Method::HEAD {
        let len = tokio::fs::metadata(&playlist_path)
            .await
            .map(|m| m.len())
            .unwrap_or(0);
        return axum::response::Response::builder()
            .status(200)
            .header("Content-Type", "application/vnd.apple.mpegurl")
            .header("Content-Length", len.to_string())
            .header("Accept-Ranges", "bytes")
            .header("Cache-Control", "no-cache")
            .header("Access-Control-Allow-Origin", "*")
            .body(Body::empty())
            .unwrap();
    }

    // Avoid holding this request open too long: Safari may leave play() pending and
    // effectively time out if the playlist GET doesn't return quickly.
    // We'll wait briefly for the playlist file to appear, then serve whatever we have.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
    let mut last_bytes: Option<Vec<u8>> = None;
    let mut saw_any_segment = false;
    let mut first_segment_name: Option<String> = None;

    while std::time::Instant::now() < deadline {
        match tokio::fs::read(&playlist_path).await {
            Ok(bytes) => {
                let text = String::from_utf8_lossy(&bytes);
                let seg_lines: Vec<String> = text
                    .lines()
                    .filter(|l| l.starts_with("seg_") && l.ends_with(".ts"))
                    .map(|s| s.to_string())
                    .collect();

                if let Some(first) = seg_lines.first() {
                    saw_any_segment = true;
                    first_segment_name = Some(first.clone());
                }
                last_bytes = Some(bytes);

                // If we already have at least one segment listed, don't block further.
                if saw_any_segment {
                    break;
                }
            }
            Err(_) => {
                // keep waiting
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }

    state.stream_manager.touch_hls(&stream_id).await;
    state.hls_manager.touch(&stream_id).await;

    match last_bytes {
        Some(bytes) => {
            // If we have a segment listed, wait briefly for the first segment to exist
            // (but don't block long enough to make Safari give up).
            if let Some(seg) = &first_segment_name {
                let seg_path = dir.join(seg);
                let seg_deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
                loop {
                    match tokio::fs::metadata(&seg_path).await {
                        Ok(m) if m.len() > 0 => break,
                        _ if std::time::Instant::now() >= seg_deadline => {
                            warn!(
                                "HLS first segment not ready (serving playlist anyway): id={} segment={} path={} ",
                                id,
                                seg,
                                seg_path.display()
                            );
                            break;
                        }
                        _ => tokio::time::sleep(std::time::Duration::from_millis(50)).await,
                    }
                }
            }

            // Rewrite relative segment URIs to absolute paths and ensure TARGETDURATION is valid.
            // Safari can be strict and may reject playlists where EXTINF exceeds TARGETDURATION.
            let text = String::from_utf8_lossy(&bytes);
            let mut max_extinf: f64 = 0.0;
            let mut current_target: Option<u64> = None;

            for line in text.lines() {
                if let Some(rest) = line.strip_prefix("#EXTINF:") {
                    if let Some((dur_str, _)) = rest.split_once(',') {
                        if let Ok(dur) = dur_str.trim().parse::<f64>() {
                            if dur > max_extinf {
                                max_extinf = dur;
                            }
                        }
                    }
                } else if let Some(rest) = line.strip_prefix("#EXT-X-TARGETDURATION:") {
                    if let Ok(v) = rest.trim().parse::<u64>() {
                        current_target = Some(v);
                    }
                }
            }

            let needed_target = max_extinf.ceil() as u64;
            let target_to_use = match current_target {
                Some(v) => v.max(needed_target),
                None => needed_target,
            };

            let mut saw_target = false;
            let mut saw_version = false;
            let rewritten = text
                .lines()
                .map(|line| {
                    if line.starts_with("#EXT-X-VERSION:") {
                        saw_version = true;
                        "#EXT-X-VERSION:3".to_string()
                    } else if line == "#EXT-X-INDEPENDENT-SEGMENTS" {
                        // Some Safari versions seem picky; this tag isn't required for TS.
                        String::new()
                    } else if line.starts_with("#EXT-X-TARGETDURATION:") {
                        saw_target = true;
                        format!("#EXT-X-TARGETDURATION:{}", target_to_use)
                    } else if line.starts_with("seg_") && line.ends_with(".ts") {
                        // Keep relative URIs (seg_XXXXX.ts). Some Safari versions appear
                        // to be picky about absolute-path URIs starting with '/'.
                        line.to_string()
                    } else {
                        line.to_string()
                    }
                })
                .collect::<Vec<_>>();

            let rewritten = if saw_target {
                rewritten
                    .into_iter()
                    .filter(|l| !l.is_empty())
                    .collect::<Vec<_>>()
                    .join("\n")
                    + "\n"
            } else {
                // Insert TARGETDURATION after EXT-X-VERSION if it was missing.
                let mut out_lines = Vec::with_capacity(rewritten.len() + 2);
                for line in rewritten {
                    if line.is_empty() {
                        continue;
                    }
                    out_lines.push(line);
                    if out_lines.last().map(|s| s.as_str()) == Some("#EXT-X-VERSION:3") {
                        out_lines.push(format!("#EXT-X-TARGETDURATION:{}", target_to_use));
                    }
                }
                if !saw_version {
                    // Extremely defensive: if VERSION was missing, add both near the top.
                    let mut with_version = Vec::with_capacity(out_lines.len() + 2);
                    for line in out_lines {
                        with_version.push(line);
                        if with_version.last().map(|s| s.as_str()) == Some("#EXTM3U") {
                            with_version.push("#EXT-X-VERSION:3".to_string());
                            with_version.push(format!("#EXT-X-TARGETDURATION:{}", target_to_use));
                        }
                    }
                    with_version.join("\n") + "\n"
                } else {
                    out_lines.join("\n") + "\n"
                }
            };

            let out = rewritten.into_bytes();

            info!(
                "Serving HLS playlist: id={} bytes={} preview=\n{}",
                id,
                out.len(),
                String::from_utf8_lossy(&out)
                    .lines()
                    .take(20)
                    .collect::<Vec<_>>()
                    .join("\n")
            );

            axum::response::Response::builder()
                .header("Content-Type", "application/vnd.apple.mpegurl")
                .header("Content-Length", out.len().to_string())
                .header("Accept-Ranges", "bytes")
                .header("Cache-Control", "no-cache")
                .header("Access-Control-Allow-Origin", "*")
                .body(Body::from(out))
                .unwrap()
        }
        None => {
            // Returning 5xx here is OK as long as the <video> element doesn't see it as its
            // primary source. The watch page probes readiness before setting src.
            warn!("HLS playlist not ready yet (no playlist file): id={} (503)", id);
            axum::response::Response::builder()
                .status(503)
                .header("Cache-Control", "no-cache")
                .header("Retry-After", "1")
                .body(Body::from("HLS not ready"))
                .unwrap()
        }
    }
}

async fn hls_segment_handler(
    Path((id, segment)): Path<(usize, String)>,
    State(state): State<Arc<AppState>>,
    method: Method,
    headers: HeaderMap,
) -> impl IntoResponse {
    if id >= state.channels.len() {
        return axum::response::Response::builder()
            .status(404)
            .body(Body::from("Channel not found"))
            .unwrap();
    }

    let channel = &state.channels[id];
    let stream_id = channel.url.clone();

    let user_agent = headers
        .get(axum::http::header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("<none>");
    let range = headers
        .get(axum::http::header::RANGE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("<none>");
    let accept = headers
        .get(axum::http::header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("<none>");
    info!(
        "HTTP HLS segment request: id={} method={} segment={} UA=\"{}\" Range=\"{}\" Accept=\"{}\"",
        id,
        method,
        segment,
        user_agent,
        range,
        accept
    );

    let dir = match state
        .hls_manager
        .get_or_start(stream_id.clone(), channel.url.clone())
        .await
    {
        Ok(d) => d,
        Err(e) => {
            return axum::response::Response::builder()
                .status(500)
                .body(Body::from(format!("Failed to start HLS: {e}")))
                .unwrap();
        }
    };

    // Ensure the single shared transcoder is running and is configured to write HLS.
    if let Err(e) = state
        .stream_manager
        .ensure_stream(stream_id.clone(), channel.url.clone(), Some(dir.clone()), Some(&state.hls_manager))
        .await
    {
        warn!("HLS ensure_stream rejected: id={} err={}", id, e);
        return axum::response::Response::builder()
            .status(503)
            .header("Cache-Control", "no-store")
            .body(Body::from(format!("Stream limit reached: {e}")))
            .unwrap();
    }
    state.stream_manager.touch_hls(&stream_id).await;
    state.hls_manager.touch(&stream_id).await;

    let Some(path) = HlsManager::segment_path(&dir, &segment) else {
        return axum::response::Response::builder()
            .status(400)
            .body(Body::from("Invalid segment"))
            .unwrap();
    };

    match tokio::fs::read(&path).await {
        Ok(bytes) => {
            let total = bytes.len();

            // Support byte ranges (some Safari/iOS HLS stacks probe with Range or require it).
            let range_header = headers
                .get(axum::http::header::RANGE)
                .and_then(|v| v.to_str().ok());

            if method == Method::HEAD {
                axum::response::Response::builder()
                    .header("Content-Type", "video/mp2t")
                    .header("Content-Length", total.to_string())
                    .header("Accept-Ranges", "bytes")
                    .header("Cache-Control", "no-store")
                    .header("Access-Control-Allow-Origin", "*")
                    .body(Body::empty())
                    .unwrap()
            } else if let Some(range_header) = range_header {
                if let Some(spec) = range_header.trim().strip_prefix("bytes=") {
                    if let Some((start_str, end_str)) = spec.split_once('-') {
                        if let (Ok(start), Ok(end)) = (start_str.parse::<usize>(), end_str.parse::<usize>()) {
                            if start <= end && end < total {
                                let body = bytes::Bytes::from(bytes[start..=end].to_vec());
                                let content_range = format!("bytes {}-{}/{}", start, end, total);
                                return axum::response::Response::builder()
                                    .status(206)
                                    .header("Content-Type", "video/mp2t")
                                    .header("Accept-Ranges", "bytes")
                                    .header("Content-Range", content_range)
                                    .header("Content-Length", body.len().to_string())
                                    .header("Cache-Control", "no-store")
                                    .header("Access-Control-Allow-Origin", "*")
                                    .body(Body::from(body))
                                    .unwrap();
                            }
                        }
                    }
                }

                // If Range was invalid/unsatisfiable, fall back to full response.
                axum::response::Response::builder()
                    .header("Content-Type", "video/mp2t")
                    .header("Content-Length", total.to_string())
                    .header("Accept-Ranges", "bytes")
                    .header("Cache-Control", "no-store")
                    .header("Access-Control-Allow-Origin", "*")
                    .body(Body::from(bytes))
                    .unwrap()
            } else {
                axum::response::Response::builder()
                    .header("Content-Type", "video/mp2t")
                    .header("Content-Length", total.to_string())
                    .header("Accept-Ranges", "bytes")
                    .header("Cache-Control", "no-store")
                    .header("Access-Control-Allow-Origin", "*")
                    .body(Body::from(bytes))
                    .unwrap()
            }
        }
        Err(_) => axum::response::Response::builder()
            .status(404)
            .body(Body::from("Segment not found"))
            .unwrap(),
    }
}

async fn stream_handler(
    Path(id): Path<usize>,
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if id >= state.channels.len() {
        return axum::response::Response::builder()
            .status(404)
            .body(Body::from("Channel not found"))
            .unwrap();
    }

    let channel = &state.channels[id];

    let user_agent = headers
        .get(axum::http::header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("<none>");
    let range = headers
        .get(axum::http::header::RANGE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("<none>");
    let accept = headers
        .get(axum::http::header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("<none>");

    info!(
        "HTTP stream request: id={} name=\"{}\" url={} UA=\"{}\" Range=\"{}\" Accept=\"{}\"",
        id,
        channel.name,
        channel.url,
        user_agent,
        range,
        accept
    );

    // Always start streams with an HLS output directory so Safari/iOS can join later
    // without requiring a second ffmpeg/RTSP session.
    let stream_id = channel.url.clone();
    let hls_dir = match state.hls_manager.get_or_start(stream_id.clone(), channel.url.clone()).await {
        Ok(d) => d,
        Err(e) => {
            return axum::response::Response::builder()
                .status(500)
                .body(Body::from(format!("Failed to prepare HLS dir: {e}")))
                .unwrap();
        }
    };
    state.hls_manager.touch(&stream_id).await;

    let (rx, header_store, cache_snapshot, guard) = match state
        .stream_manager
        .get_or_start_stream(stream_id.clone(), channel.url.clone(), Some(hls_dir), Some(&state.hls_manager))
        .await
    {
        Ok(v) => v,
        Err(e) => {
            warn!("Stream rejected (capacity?): id={} err={}", id, e);
            return axum::response::Response::builder()
                .status(503)
                .header("Cache-Control", "no-store")
                .body(Body::from(format!("Stream limit reached: {e}")))
                .unwrap();
        }
    };

    // Wait for header
    let mut header_data = None;
    for _ in 0..150 { // Wait up to 15 seconds for transcoding to start
        {
            let h = header_store.read().await;
            if let Some(ref data) = *h {
                header_data = Some(data.clone());
                break;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    let header = match header_data {
        Some(h) => h,
        None => {
            return axum::response::Response::builder()
                .status(504)
                .body(Body::from("Timeout starting stream"))
                .unwrap();
        }
    };

    // iOS Safari (and some embedded clients) often probe MP4 streams with a tiny
    // fixed Range request (e.g. `bytes=0-1`) before attempting playback.
    // We can't do real byte serving for an infinite live stream, but we *can*
    // satisfy small fixed ranges out of the already-captured MP4 header.
    if let Some(range_header) = headers.get(axum::http::header::RANGE).and_then(|v| v.to_str().ok()) {
        if let Some(spec) = range_header.trim().strip_prefix("bytes=") {
            if let Some((start_str, end_str)) = spec.split_once('-') {
                if let (Ok(start), Ok(end)) = (start_str.parse::<usize>(), end_str.parse::<usize>()) {
                    if start <= end {
                        let total = header.len();
                        if end < total {
                            let body_bytes = header.slice(start..=end);
                            let content_range = format!("bytes {}-{}/{}", start, end, total);
                            info!(
                                "Serving header range: id={} Range=\"{}\" -> {} (len={})",
                                id,
                                range_header,
                                content_range,
                                body_bytes.len()
                            );
                            return axum::response::Response::builder()
                                .status(206)
                                .header("Content-Type", "video/mp4")
                                .header("Accept-Ranges", "bytes")
                                .header("Content-Range", content_range)
                                .header("Content-Length", body_bytes.len().to_string())
                                .header("Cache-Control", "no-store")
                                .body(Body::from(body_bytes))
                                .unwrap();
                        } else {
                            let content_range = format!("bytes */{}", total);
                            warn!(
                                "Unsatisfiable range (header only): id={} Range=\"{}\" header_len={}",
                                id,
                                range_header,
                                total
                            );
                            return axum::response::Response::builder()
                                .status(416)
                                .header("Content-Range", content_range)
                                .header("Cache-Control", "no-store")
                                .body(Body::empty())
                                .unwrap();
                        }
                    }
                }
            }
        }
    }

    let cache_chunks = cache_snapshot.len();
    let cache_bytes: usize = cache_snapshot.iter().map(|b| b.len()).sum();
    info!(
        "Stream start: id={} cache_chunks={} cache_bytes={}",
        id,
        cache_chunks,
        cache_bytes
    );

    // Combine header + cache + broadcast stream
    // Use an explicit recv() loop so we can log when the broadcast stream ends.
    let id_for_logs = std::sync::Arc::new(id.clone());
    let broadcast_stream = futures::stream::unfold(rx, move |mut rx| {
        let id_for_logs = std::sync::Arc::clone(&id_for_logs);
        async move {
            loop {
                match rx.recv().await {
                    Ok(bytes) => return Some((Ok::<_, std::io::Error>(bytes), rx)),
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(skipped)) => {
                        warn!(
                            "Stream lagged: id={} skipped_messages={}",
                            id_for_logs,
                            skipped
                        );
                        continue;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        warn!("Stream ended (broadcast closed): id={}", id_for_logs);
                        return None;
                    }
                }
            }
        }
    });
    
    // Create cache stream
    let cache_stream = futures::stream::iter(cache_snapshot)
        .map(|b| Ok::<_, std::io::Error>(b));

    let stream = futures::stream::once(async move { Ok::<_, std::io::Error>(header) })
        .chain(cache_stream)
        .chain(broadcast_stream);

    // Keep the client guard alive for as long as the HTTP body is alive.
    let guarded_stream = GuardedStream {
        _guard: guard,
        inner: Box::pin(stream),
        id: id,
        last_log_time: std::time::Instant::now(),
        bytes_since_last_log: 0,
    };

    axum::response::Response::builder()
        .header("Content-Type", "video/mp4")
        .header("Cache-Control", "no-store")
        .body(Body::from_stream(guarded_stream))
        .unwrap()
}
