use crate::transcoder::TuningMode;
use tracing::info;

pub fn detect_auto() -> String {
    // VideoToolbox is always available on modern macOS.
    info!("Auto-detected macOS. Using 'videotoolbox' mode.");
    "videotoolbox".to_string()
}

pub fn get_args_videotoolbox(mode: TuningMode) -> Vec<String> {
    let mut args = vec![
        "-c:v".into(), "h264_videotoolbox".into(),
        // VideoToolbox Rate Control:
        // -b:v sets the target average.
        // -maxrate guards against spikes (crucial for streaming).
        "-b:v".into(), "6M".into(),
        "-maxrate".into(), "8M".into(),
        "-bufsize".into(), "8M".into(),
        
        // Compatibility:
        // 'high' profile is widely supported on modern Apple Silicon and iOS > 10.
        // It offers better compression than baseline/main.
        "-profile:v".into(), "high".into(),
        
        // Allow automatic software fallback if HW runs out of instances, 
        // though unlikely on M-series chips.
        "-allow_sw".into(), "1".into(),
    ];

    match mode {
        TuningMode::LowLatency => {
            args.extend([
                // Crucial for low-latency:
                "-realtime".into(), "true".into(), 
                // Don't reorder frames = 0 latency from B-frames
                "-bf".into(), "0".into(), 
            ]);
        }
        TuningMode::Smooth => {
            // Allows B-frames for better quality/bitrate ratio
            args.extend([
                "-realtime".into(), "true".into(), 
                // Prioritize quality over strict latency
                "-prio_speed".into(), "false".into(), 
            ]);
        }
    }

    args
}
