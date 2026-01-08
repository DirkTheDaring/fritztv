use crate::transcoder::TuningMode;
use tracing::info;

pub fn detect_auto() -> String {
    // Robust detection requires running 'ffmpeg -encoders' or finding DLLs.
    // For now, default to CPU to be safe, or we could blindly try 'amf' if on AMD.
    // A simplified approach:
    info!("Windows auto-detection not fully implemented. Defaulting to 'cpu'. Set 'amf', 'nvenc', or 'qsv' manually in config.toml if you have a GPU.");
    "cpu".to_string()
}

pub fn get_args_amf(mode: TuningMode) -> Vec<String> {
    let mut args = vec![
        "-c:v".into(), "h264_amf".into(),
        // AMF (Advanced Media Framework) for AMD GPUs
        // Enforce CBR for streaming stability
        "-rc".into(), "cbr".into(),
        "-b:v".into(), "6M".into(),
        "-maxrate".into(), "6M".into(),
        "-bufsize".into(), "6M".into(),
    ];
    match mode {
        TuningMode::LowLatency => {
            args.extend([
                "-usage".into(), "lowlatency".into(),
                "-quality".into(), "speed".into(),
            ]);
        }
        TuningMode::Smooth => {
            args.extend([
                "-usage".into(), "transcoding".into(),
                "-quality".into(), "balanced".into(),
            ]);
        }
    }
    args
}

pub fn get_args_nvenc(mode: TuningMode) -> Vec<String> {
    let mut args = vec![
        "-c:v".into(), "h264_nvenc".into(),
        // NVENC (NVIDIA)
        // CBR usually preferred for strict streaming
        "-rc".into(), "cbr".into(),
        "-b:v".into(), "6M".into(),
        "-maxrate".into(), "6M".into(),
        "-bufsize".into(), "6M".into(),
    ];
    match mode {
        TuningMode::LowLatency => {
            args.extend([
                "-preset".into(), "p2".into(), // p1 is too blocky, p2 is good compromise
                "-tune".into(), "ull".into(),  // Ultra Low Latency
                "-zerolatency".into(), "1".into(),
                "-delay".into(), "0".into(),
            ]);
        }
        TuningMode::Smooth => {
            args.extend(["-preset".into(), "p4".into()]);
        }
    }
    args
}

pub fn get_args_qsv(mode: TuningMode) -> Vec<String> {
     let mut args = vec![
        "-c:v".into(), "h264_qsv".into(),
        // Intel QSV
        // VBR is safe usually, but 'cbr' is stricter
        "-b:v".into(), "6M".into(),
        "-maxrate".into(), "6M".into(),
        "-bufsize".into(), "12M".into(), // Intel likes larger buffer for VBR
    ];
    if mode == TuningMode::LowLatency {
         args.extend([
             "-look_ahead".into(), "0".into(),
             "-async_depth".into(), "1".into(),
         ]);
    }
    args
}
