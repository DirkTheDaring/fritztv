# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.7.3] - 2026-01-08
### Fixed
- **Packaging**: Updated systemd service file to allow access to `/dev/dri` (render/video groups) for VAAPI hardware acceleration.

## [0.7.2] - 2026-01-08
### Added
- **Hardware Acceleration**: Added `transcoding.hw_accel` configuration (options: `cpu` [default], `vaapi` for AMD/Intel GPUs).

## [0.7.1] - 2026-01-08
### Fixed
- **Build**: Fixed a compilation error on macOS (and other platforms) caused by a temporary value drop in `sysinfo` integration.

## [0.7.0] - 2026-01-08
### Added
- **Flexible Multithreading**: Added `transcoding.threads` config to tune FFmpeg CPU usage (default: auto).
- **Prometheus Monitoring**: Exposed `/metrics` endpoint including:
    - `fritztv_client_bandwidth_bytes`: Per-client bandwidth usage.
    - `fritztv_ffmpeg_cpu_usage_percent`: Per-channel FFmpeg CPU load.
- **Configurable Logging**: Added `monitoring.console_log_bandwidth` to toggle verbose console output.

### Changed
- **Optimized HLS**: Replaced polling loops with `inotify` (via `notify` crate) for instant, zero-CPU HLS playlist updates.

## [0.6.0] - 2026-01-08

### Added
- **Configurable Idle Timeout**: New `transcoding.idle_timeout` setting (default: 10s) to quickly free up tuners after clients disconnect.
- **Bandwidth Tracking**: Server now logs per-client data transfer rates every 5 seconds.
- **Stream Stabilization**: Added `genpts` and `discardcorrupt` to FFmpeg inputs to fix initial video scrambling/lag.
- **iOS Compatibility**: Enforced `Closed GOP` (+cgop) and `make_zero` timestamps to ensure Apple HLS players work reliably.

### Fixed
- **64-bit Atom Crash**: Fixed a bug where massive fMP4 atoms (size > 4GB or extended header) could cause the transcoder to panic.
- **HLS Cleanup**: Fixed an issue where restarting a stream reused dirty HLS directories, causing playback glitches. Directories are now explicitly cleaned on new sessions.

## [0.5.0] - 2026-01-07

### Added
- **RTSP Transcoding**: Full support for ingesting DVB-C RTSP streams from FritzBox Cable.
- **Universal Sync Fix**: `Smooth` mode now includes `aresample=async=1` and `-vsync 1` to solve audio/video sync issues on Linux browsers (Chrome/Firefox) while preserving iOS compatibility.
- **Web UI**: Responsive interface for channel browsing and playback.
- **HLS & fMP4**: Support for both HTTP Live Streaming (Apple) and fragmented MP4 (MSE).
- **Cross-Platform Builds**: Makefile targets for Windows (`.exe`), macOS, and Linux binaries.
- **Packaging**: Automated RPM and Debian package generation.
