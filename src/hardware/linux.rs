use crate::transcoder::TuningMode;
use tracing::{info, warn};
use std::path::Path;

pub fn detect_auto() -> String {
    let path = Path::new("/dev/dri/renderD128");
    if path.exists() {
        match std::fs::File::open(path) {
            Ok(_) => {
                info!("Auto-detected VAAPI device at {:?}. Using 'vaapi' mode.", path);
                "vaapi".to_string()
            }
            Err(e) => {
                warn!("Auto-detection: VAAPI device found at {:?} but cannot be opened ({}). Falling back to 'cpu'. Check user permissions (render group?) or systemd DeviceAllow.", path, e);
                "cpu".to_string()
            }
        }
    } else {
        info!("Auto-detection: No VAAPI device found at {:?}. Using 'cpu' mode.", path);
        "cpu".to_string()
    }
}

pub fn get_global_args_vaapi() -> Vec<String> {
    vec![
        "-init_hw_device".into(), "vaapi=va:/dev/dri/renderD128".into(),
        "-filter_hw_device".into(), "va".into(),
    ]
}

pub fn get_args_vaapi(mode: TuningMode) -> Vec<String> {
    let mut args = vec![
        // Filter Chain:
        // 1. format=nv12: Ensure correct pixel format for hardware.
        // 2. hwupload: Move frame to GPU memory.
        // 3. deinterlace_vaapi: CRITICAL for DVB signals (1080i/576i). 
        //    Without this, sports/tickers will look terrible (combing).
        //    'rate=field' (default) doubles framerate (50i -> 50p) for smooth motion.
        "-vf".into(), "format=nv12,hwupload,deinterlace_vaapi".into(),
        
        "-c:v".into(), "h264_vaapi".into(),
        
        // Bitrate Control:
        // Replaced fixed QP with VBR (Variable Bit Rate) + Caps.
        // QP is dangerous for streaming; noise/grain can cause 50Mbps+ spikes, stalling clients.
        "-b:v".into(), "6M".into(),
        "-maxrate".into(), "8M".into(),
        "-bufsize".into(), "8M".into(),
    ];

    match mode {
        TuningMode::LowLatency => {
            args.extend([
                // Disable B-frames for lowest latency (no reordering delay)
                "-bf".into(), "0".into(),
            ]);
        }
        TuningMode::Smooth => {
            // Default VAAPI usually allows B-frames (driver dependent), 
            // helpful for quality at same bitrate.
        }
    }

    args
}
