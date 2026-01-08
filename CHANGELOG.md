# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
