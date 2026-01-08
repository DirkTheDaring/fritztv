use crate::transcoder::TuningMode;

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "windows")]
pub mod windows;

pub mod cpu;

pub fn detect(configured_mode: Option<String>) -> String {
    let mode = configured_mode.unwrap_or_else(|| "auto".to_string());
    if mode == "cpu" || mode != "auto" {
        return mode;
    }

    #[cfg(target_os = "linux")]
    { return linux::detect_auto(); }

    #[cfg(target_os = "macos")]
    { return macos::detect_auto(); }

    #[cfg(target_os = "windows")]
    { return windows::detect_auto(); }

    // Fallback for other OSs (BSD, etc)
    #[allow(unreachable_code)]
    "cpu".to_string()
}

pub fn get_ffmpeg_args(hw_accel: &str, mode: TuningMode, threads: u8) -> Vec<String> {
    if hw_accel == "cpu" {
        return cpu::get_args(mode, threads);
    }

    #[cfg(target_os = "linux")]
    if hw_accel == "vaapi" {
        return linux::get_args_vaapi(mode);
    }

    #[cfg(target_os = "macos")]
    if hw_accel == "videotoolbox" {
        return macos::get_args_videotoolbox(mode);
    }
    
    // Windows specific modes
    #[cfg(target_os = "windows")]
    {
         if hw_accel == "amf" { return windows::get_args_amf(mode); }
         if hw_accel == "nvenc" { return windows::get_args_nvenc(mode); }
         if hw_accel == "qsv" { return windows::get_args_qsv(mode); }
    }

    // Fallback if unknown mode passed or OS mismatch
    cpu::get_args(mode, threads)
}

pub fn get_global_args(hw_accel: &str) -> Vec<String> {
    #[cfg(target_os = "linux")]
    if hw_accel == "vaapi" {
        return linux::get_global_args_vaapi();
    }
    
    Vec::new()
}
