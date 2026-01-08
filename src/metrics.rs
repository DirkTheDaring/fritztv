use lazy_static::lazy_static;
use prometheus::{register_gauge_vec, GaugeVec, Encoder, TextEncoder};
use serde::Deserialize;

lazy_static! {
    pub static ref CLIENT_BANDWIDTH: GaugeVec = register_gauge_vec!(
        "fritztv_client_bandwidth_bytes",
        "Current bandwidth usage per client in bytes/sec",
        &["channel_id"]
    )
    .unwrap();
    pub static ref FFMPEG_CPU_USAGE: GaugeVec = register_gauge_vec!(
        "fritztv_ffmpeg_cpu_usage_percent",
        "Current CPU usage of the ffmpeg process per channel (0-100+)",
        &["channel_id"]
    )
    .unwrap();
}

pub fn gather_metrics() -> String {
    let mut buffer = Vec::new();
    let encoder = TextEncoder::new();
    let metric_families = prometheus::gather();
    encoder.encode(&metric_families, &mut buffer).unwrap();
    String::from_utf8(buffer).unwrap()
}

#[derive(Debug, Deserialize, Clone)]
pub struct MonitoringConfig {
    #[serde(default = "default_monitoring_enabled")]
    pub enabled: bool,
    #[serde(default = "default_console_log_bandwidth")]
    pub console_log_bandwidth: bool,
}

fn default_monitoring_enabled() -> bool {
    true
}

fn default_console_log_bandwidth() -> bool {
    false
}
