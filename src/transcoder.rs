use tokio::process::Command;
use tokio::sync::broadcast;
use tokio::io::AsyncReadExt;
use std::process::Stdio;
use std::sync::Arc;
use std::path::PathBuf;
use tokio::sync::RwLock;
use tracing::{info, warn, error, debug};
use bytes::{Bytes, BytesMut};
use tokio::sync::Mutex;
use std::collections::VecDeque;
use sysinfo::{Pid, System};
use crate::metrics::FFMPEG_CPU_USAGE;

pub struct Transcoder {
    stop_signal: tokio::sync::watch::Sender<bool>,
    channel_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TuningMode {
    LowLatency,
    Smooth,
}

impl Transcoder {
    pub fn new(
        channel_id: String,
        url: String,
        tx: broadcast::Sender<Bytes>,
        header_store: Arc<RwLock<Option<Bytes>>>,
        mode: TuningMode,
        transport: String,

        hls_dir: Option<PathBuf>,
        threads: u8,
    ) -> Self {
        let (stop_tx, mut stop_rx) = tokio::sync::watch::channel(false);
        let channel_id_task = channel_id.clone();

        tokio::spawn(async move {
            let channel_id = channel_id_task; // Shadow it for convenience inside the task
            info!(
                "Starting ffmpeg for {} in {:?} mode (transport: {}, hls={})",
                url,
                mode,
                transport,
                hls_dir.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "off".to_string())
            );
            
            let mut args: Vec<String> = Vec::new();

            if transport == "tcp" {
                args.push("-rtsp_transport".into());
                args.push("tcp".into());
            }

            // Input-side buffering can help with UDP/RTP jitter.
            args.extend(["-rtbufsize".into(), "10M".into()]);

            // Robustness: clean up input timestamps and drop garbage.
            args.extend([
                "-fflags".into(), "+genpts+discardcorrupt".into(),
                "-avoid_negative_ts".into(), "make_zero".into(),
            ]);

            // Tuning for startup speed vs stability
            match mode {
                TuningMode::LowLatency => {
                    args.extend([
                        "-analyzeduration".into(), "2000000".into(), // 2s
                        "-probesize".into(), "2000000".into(),       // 2MB
                    ]);
                }
                TuningMode::Smooth => {
                    args.extend([
                        "-analyzeduration".into(), "10000000".into(), // 10s
                        "-probesize".into(), "10000000".into(),       // 10MB
                    ]);
                }
            }

            args.push("-y".into());
            args.push("-i".into());
            args.push(url.clone());

            // IMPORTANT: In ffmpeg, most output/codec options apply only to the *next* output.
            // Because we generate MP4 *and* HLS in one process, we must set mapping/codec
            // options separately for each output.
            let push_output_av_settings = |out: &mut Vec<String>| {
                // Only include A/V in the output. Fritzbox DVB streams often contain
                // teletext/subtitle/data tracks that can make ffmpeg abort if auto-mapped.
                out.extend([
                    "-map".into(), "0:v:0".into(),
                    "-map".into(), "0:a:0?".into(),
                    "-sn".into(),
                    "-dn".into(),
                    // Universal Sync Fix for Linux Browsers:
                    // 1. Force audio resampling to match timestamps (fixes drift)
                    "-af".into(), "aresample=async=1".into(),
                    // 2. Enforce constant frame rate (helps browser MSE stability)
                    "-vsync".into(), "1".into(),
                    // 3. Allow larger muxing queue for jittery RTSP inputs
                    "-max_muxing_queue_size".into(), "1024".into(),
                ]);

                out.extend([
                    "-vf".into(), "yadif".into(),
                    "-pix_fmt".into(), "yuv420p".into(),

                    "-c:v".into(), "libx264".into(),
                    "-threads".into(), threads.to_string(),
                    // Baseline profile for iOS compatibility.
                    "-profile:v".into(), "baseline".into(),
                    "-level".into(), "3.1".into(),
                    // HLS Requirement: Closed GOPs for independent segments
                    "-flags".into(), "+cgop".into(),
                    // Make keyframes predictable to reduce client buffering and align
                    // fMP4 fragments / HLS segments with IDR boundaries.
                    "-g".into(), "50".into(),
                    "-keyint_min".into(), "50".into(),
                    "-sc_threshold".into(), "0".into(),
                    // Force an IDR roughly every 2s regardless of input fps.
                    "-force_key_frames".into(), "expr:gte(t,n_forced*2)".into(),
                    "-crf".into(), "18".into(),
                    "-maxrate".into(), "12M".into(),
                    "-bufsize".into(), "24M".into(),
                    "-c:a".into(), "aac".into(),
                    "-ac".into(), "2".into(),
                    "-b:a".into(), "128k".into(),
                ]);

                match mode {
                    TuningMode::LowLatency => {
                        out.extend([
                            "-preset".into(), "fast".into(),
                            "-tune".into(), "zerolatency".into(),
                        ]);
                    }
                    TuningMode::Smooth => {
                        out.extend([
                            // Smooth: stable and CPU-friendly; avoid periodic encoder stalls.
                            "-preset".into(), "medium".into(),
                        ]);
                    }
                }
            };

            // Output 1: fMP4 to stdout.
            push_output_av_settings(&mut args);
            args.extend([
                "-f".into(), "mp4".into(),
                "-movflags".into(), "frag_keyframe+empty_moov+default_base_moof".into(),
                "pipe:1".into(),
            ]);

            // Output 2 (optional): HLS to disk, for iOS/Safari.
            if let Some(dir) = &hls_dir {
                let seg_pat = dir.join("seg_%05d.ts").to_string_lossy().to_string();
                let playlist = dir.join("index.m3u8").to_string_lossy().to_string();
                push_output_av_settings(&mut args);
                args.extend([
                    "-mpegts_flags".into(), "+resend_headers".into(),
                    "-f".into(), "hls".into(),
                    "-hls_time".into(), "2".into(),
                    "-hls_list_size".into(), "10".into(),
                    "-hls_playlist_type".into(), "event".into(),
                    "-hls_flags".into(), "delete_segments+independent_segments+omit_endlist".into(),
                    "-hls_segment_filename".into(), seg_pat,
                    playlist,
                ]);
            }

            let child = Command::new("ffmpeg")
                .args(&args)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true)
                .spawn();

            match child {
                Ok(mut child) => {
                    if let Some(pid) = child.id() {
                        info!("ffmpeg spawned: pid={} url={}", pid, url);
                        
                        // CPU Monitoring Task
                        let channel_id_mon = channel_id.clone();
                        let pid_u32 = pid;
                        let mut stop_rx_mon = stop_rx.clone();
                        
                        tokio::spawn(async move {
                            let mut sys = System::new();
                            let pid = Pid::from_u32(pid_u32);
                            
                            loop {
                                tokio::select! {
                                    _ = stop_rx_mon.changed() => break,
                                    _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => {
                                        let processes = sysinfo::ProcessesToUpdate::Some(&[pid]);
                                        sys.refresh_processes(processes, true);
                                        if let Some(process) = sys.process(pid) {
                                            let usage = process.cpu_usage();
                                            FFMPEG_CPU_USAGE.with_label_values(&[&channel_id_mon]).set(usage as f64);
                                        } else {
                                            break; 
                                        }
                                    }
                                }
                            }
                        });
                    }
                    let mut stdout = child.stdout.take().expect("Failed to open stdout");
                    let stderr = child.stderr.take().expect("Failed to open stderr");

                    // Capture a rolling buffer of stderr lines so we can print context
                    // when ffmpeg exits, without spamming the console.
                    let stderr_ring: Arc<Mutex<VecDeque<String>>> = Arc::new(Mutex::new(VecDeque::with_capacity(80)));
                        let stderr_ring_for_reader = Arc::clone(&stderr_ring);
                        tokio::spawn(async move {
                            let mut buffer = String::new();

                        let mut reader = tokio::io::BufReader::new(stderr);
                        use tokio::io::AsyncBufReadExt;
                        while let Ok(n) = reader.read_line(&mut buffer).await {
                            if n == 0 {
                                break;
                            }
                            let line = buffer.trim().to_string();
                            {
                                let mut ring = stderr_ring_for_reader.lock().await;
                                if ring.len() >= 50 {
                                    ring.pop_front();
                                }
                                ring.push_back(line.clone());
                            }
                            // Keep ffmpeg output available but quiet by default.
                            // Run with `RUST_LOG=fritztv::transcoder=debug` to see it.
                            debug!("ffmpeg: {}", line);
                            buffer.clear();
                        }
                    });

                    let mut buffer = [0u8; 64 * 1024];
                    let mut stream_buffer = BytesMut::new();
                    let mut header_buffer = BytesMut::new();
                    let mut header_captured = false;
                    // Once header is captured, we package atoms into full fMP4 fragments.
                    // Broadcasting individual atoms is fragile: if a receiver lags and drops a
                    // single atom, playback can stall. Broadcasting complete fragments (moof +
                    // following atoms, typically mdat) makes lag/drop behavior much more robust.
                    let mut fragment_buffer = BytesMut::new();

                    let mut stop_requested = false;
                    let mut saw_stdout_eof = false;
                    loop {
                        tokio::select! {
                            _ = stop_rx.changed() => {
                                stop_requested = true;
                                let _ = child.kill().await;
                                break;
                            }
                            read_result = stdout.read(&mut buffer) => {
                                match read_result {
                                    Ok(0) => {
                                        saw_stdout_eof = true;
                                        break;
                                    }
                                    Ok(n) => {
                                        stream_buffer.extend_from_slice(&buffer[..n]);

                                        loop {
                                            // Check if we have enough bytes for atom header (8 bytes)
                                            if stream_buffer.len() < 8 {
                                                break;
                                            }

                                            let mut size = u32::from_be_bytes(stream_buffer[0..4].try_into().unwrap()) as usize;
                                            let mut header_len = 8;

                                            // Extended size support
                                            if size == 1 {
                                                if stream_buffer.len() < 16 {
                                                    break;
                                                }
                                                let huge_size = u64::from_be_bytes(stream_buffer[8..16].try_into().unwrap());
                                                // usize might be 32-bit on some systems, though unlikely for this server.
                                                // Cap at rational limits for fMP4 fragments (e.g. 100MB).
                                                if huge_size > 100 * 1024 * 1024 {
                                                    error!("Atom size too large: {} (url={})", huge_size, url);
                                                    break;
                                                }
                                                size = huge_size as usize;
                                                header_len = 16;
                                            } else if size < 8 {
                                                error!("Invalid atom size: {} (url={})", size, url);
                                                break;
                                            }

                                            if stream_buffer.len() < size {
                                                // Not enough data for full atom
                                                break;
                                            }

                                            // Extract the full atom
                                            let atom_data = stream_buffer.split_to(size).freeze();
                                            let type_offset = if header_len == 16 { 4 } else { 4 };
                                            let type_str = std::str::from_utf8(&atom_data[type_offset..type_offset+4]).unwrap_or("????");

                                            if !header_captured {
                                                if type_str == "moof" {
                                                    // This is the first fragment! Header is complete.
                                                    {
                                                        let mut w = header_store.write().await;
                                                        *w = Some(header_buffer.clone().freeze());
                                                    }
                                                    info!("Header captured! Size: {}", header_buffer.len());
                                                    header_captured = true;

                                                    // Start first fragment with this moof
                                                    fragment_buffer.extend_from_slice(&atom_data);
                                                } else {
                                                    // Keep adding to header
                                                    header_buffer.extend_from_slice(&atom_data);
                                                }
                                            } else {
                                                // Header already captured: package into fragments.
                                                if type_str == "moof" {
                                                    // If we see a new moof while the previous fragment
                                                    // wasn't flushed (unexpected but possible), flush it.
                                                    if !fragment_buffer.is_empty() {
                                                        let _ = tx.send(fragment_buffer.split().freeze());
                                                    }
                                                    fragment_buffer.extend_from_slice(&atom_data);
                                                } else {
                                                    if fragment_buffer.is_empty() {
                                                        // We expect fragments to start with moof. If we don't have one,
                                                        // drop data until the next moof to avoid sending invalid fragments.
                                                        continue;
                                                    }

                                                    fragment_buffer.extend_from_slice(&atom_data);

                                                    // Typical fMP4 fragment ends after mdat.
                                                    if type_str == "mdat" {
                                                        let _ = tx.send(fragment_buffer.split().freeze());
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        error!("Error reading ffmpeg stdout: {} (url={})", e, url);
                                        break;
                                    }
                                }
                            }
                        }
                    }

                    // Wait for ffmpeg to actually exit and report status.
                    match child.wait().await {
                        Ok(status) => {
                            if stop_requested {
                                info!("ffmpeg stopped (requested): url={} status={}", url, status);
                            } else if status.success() {
                                warn!("ffmpeg exited successfully but unexpectedly: url={} status={} saw_stdout_eof={}", url, status, saw_stdout_eof);
                            } else {
                                let ring = stderr_ring.lock().await;
                                if ring.is_empty() {
                                    warn!("ffmpeg exited with error: url={} status={} (no stderr captured)", url, status);
                                } else {
                                    warn!(
                                        "ffmpeg exited with error: url={} status={} last_stderr_lines=\n{}",
                                        url,
                                        status,
                                        ring.iter().cloned().collect::<Vec<_>>().join("\n")
                                    );
                                }
                            }

                        }
                        Err(e) => {
                            warn!("ffmpeg wait() failed: url={} err={}", url, e);
                        }
                    }
                }
                Err(e) => {
                    error!("Failed to spawn ffmpeg: {}", e);
                }
            }
        });

        Self {
            stop_signal: stop_tx,
            channel_id, 
        }
    }
}

impl Drop for Transcoder {
    fn drop(&mut self) {
        let _ = self.stop_signal.send(true);
        FFMPEG_CPU_USAGE.with_label_values(&[&self.channel_id]).set(0.0);
    }
}
