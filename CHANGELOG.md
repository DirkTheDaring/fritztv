# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.5.0] - 2026-01-07

### Added
- **RTSP Transcoding**: Full support for ingesting DVB-C RTSP streams from FritzBox Cable.
- **Universal Sync Fix**: `Smooth` mode now includes `aresample=async=1` and `-vsync 1` to solve audio/video sync issues on Linux browsers (Chrome/Firefox) while preserving iOS compatibility.
- **Web UI**: Responsive interface for channel browsing and playback.
- **HLS & fMP4**: Support for both HTTP Live Streaming (Apple) and fragmented MP4 (MSE).
- **Cross-Platform Builds**: Makefile targets for Windows (`.exe`), macOS, and Linux binaries.
- **Packaging**: Automated RPM and Debian package generation.
