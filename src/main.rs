use fritztv::{create_app, fetch_channels, channels::Channel, transcoder::TuningMode};
use tracing::{info, error};
use clap::Parser;
use config::Config;
use serde::Deserialize;
use serde::de::Deserializer;

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// Latency tuning mode (overrides config)
    #[arg(long)]
    mode: Option<ModeArg>,

    /// Path to configuration file
    #[arg(long, default_value = "config.toml")]
    config: String,
}

#[derive(clap::ValueEnum, Clone, Debug, Deserialize)]
enum ModeArg {
    LowLatency,
    Smooth,
}

#[derive(Debug, Deserialize)]
struct Settings {
    server: ServerConfig,
    fritzbox: FritzboxConfig,
    transcoding: TranscodingConfig,
}

#[derive(Debug, Deserialize)]
struct ServerConfig {
    host: String,
    port: u16,
    #[serde(default = "default_max_parallel_streams")]
    max_parallel_streams: usize,
}

fn default_max_parallel_streams() -> usize {
    4
}

#[derive(Debug, Deserialize)]
struct FritzboxConfig {
    #[serde(alias = "playlist_url", deserialize_with = "deserialize_one_or_many")]
    playlist_urls: Vec<String>,
}

fn deserialize_one_or_many<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum OneOrMany {
        One(String),
        Many(Vec<String>),
    }

    match OneOrMany::deserialize(deserializer)? {
        OneOrMany::One(s) => Ok(vec![s]),
        OneOrMany::Many(v) => Ok(v),
    }
}

#[derive(Debug, Deserialize)]
struct TranscodingConfig {
    mode: ModeArg,
    #[serde(default = "default_transport")]
    transport: String,
}

fn default_transport() -> String {
    "udp".to_string()
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
    
    let args = Args::parse();

    // Load configuration
    let settings = Config::builder()
        .add_source(config::File::with_name(&args.config))
        .build()?;
    let settings: Settings = settings.try_deserialize()?;
    
    info!("Configuration loaded from {}: {:?}", args.config, settings);
    
    let tuning_mode_arg = args.mode.unwrap_or(settings.transcoding.mode);

    let tuning_mode = match tuning_mode_arg {
        ModeArg::LowLatency => TuningMode::LowLatency,
        ModeArg::Smooth => TuningMode::Smooth,
    };

    info!("Starting server in {:?} mode (transport: {})", tuning_mode, settings.transcoding.transport);

    let mut channels: Vec<Channel> = Vec::new();
    for playlist_url in &settings.fritzbox.playlist_urls {
        info!("Fetching channel list from {}...", playlist_url);
        match fetch_channels(playlist_url).await {
            Ok(mut c) => {
                info!("Loaded {} channels from {}", c.len(), playlist_url);
                channels.append(&mut c);
            }
            Err(e) => {
                error!("Failed to fetch channels from {}: {}", playlist_url, e);
            }
        }
    }

    if channels.is_empty() {
        error!("No channels loaded from any playlist. Using a mock channel for safety.");
        channels = vec![Channel {
            name: "Test Channel".to_string(),
            url: "rtsp://127.0.0.1:8554/test".to_string(),
        }];
    }

    info!("Total loaded channels: {}", channels.len());

    let app = create_app(
        channels,
        tuning_mode,
        settings.transcoding.transport,
        settings.server.max_parallel_streams,
    );

    let addr = format!("{}:{}", settings.server.host, settings.server.port);
    info!("Listening on http://{}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
