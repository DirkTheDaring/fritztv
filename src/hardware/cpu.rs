use crate::transcoder::TuningMode;

pub fn get_args(mode: TuningMode, threads: u8) -> Vec<String> {
    let mut args = Vec::new();
    
    args.extend([
        "-vf".into(), "yadif".into(),
        "-pix_fmt".into(), "yuv420p".into(),
        "-c:v".into(), "libx264".into(),
        "-crf".into(), "18".into(),
        "-threads".into(), threads.to_string(),
        "-profile:v".into(), "baseline".into(),
        "-level".into(), "3.1".into(),
    ]);

    match mode {
        TuningMode::LowLatency => {
            args.extend(["-preset".into(), "fast".into(), "-tune".into(), "zerolatency".into()]);
        }
        TuningMode::Smooth => {
            args.extend(["-preset".into(), "medium".into()]);
        }
    }
    
    args
}
