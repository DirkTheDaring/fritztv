# Fritztv

[![Release](https://img.shields.io/github/v/release/dietmar/fritztv?include_prereleases&style=flat-square)](https://github.com/dietmar/fritztv/releases)
[![Build Status](https://img.shields.io/github/actions/workflow/status/dietmar/fritztv/release.yml?branch=master&style=flat-square)](https://github.com/dietmar/fritztv/actions)
[![License](https://img.shields.io/badge/license-MIT-blue?style=flat-square)](LICENSE)

**Fritztv** is a high-performance, lightweight transcoding server designed to bridge FritzBox Cable TV tuners with modern web browsers and mobile devices.

It consumes raw RTSP streams (DVB-C) from a generic FritzBox Cable router and transcodes them on-the-fly into browser-compatible formats, solving common audio/video synchronization issues across different platforms.

## 🚀 Features

*   **Universal Compatibility**: 
    *   **Linux Desktop**: Solves audio/video desync issues in Chrome and Firefox using advanced audio resampling and timestamp correction.
    *   **Apple Devices**: Fully compatible with iOS, iPadOS, and macOS (Safari) using standard H.264 profiles.
    *   **Smart TV/Android**: Works with any browser supporting MSE or HLS.
*   **Low Latency & Smooth Modes**: Choose between ultra-low latency for sports or smooth buffering for cinema.
*   **Integrated Web UI**: Clean, responsive interface to browse channels and watch TV directly in your browser.
*   **Efficient Transcoding**: Powered by `ffmpeg` with optimized presets for software encoding.
*   **Robust Packaging**: Easy installation via RPM, Debian packages, or Docker.

## 📋 Requirements

*   **Hardware**: A Linux server (x86_64 or ARM64) strong enough to run software transcoding (e.g., Raspberry Pi 4 is borderline, standard PC/Server recommended).
*   **Software**: 
    *   `ffmpeg` must be installed and in the system PATH.
        > **⚠️ Important for Fedora Users**: The default `ffmpeg-free` package in Fedora lacks H.264/AAC support. You **must** install the full version from [RPM Fusion](https://rpmfusion.org/):
        > ```bash
        > # 1. Enable RPM Fusion
        > sudo dnf install https://mirrors.rpmfusion.org/free/fedora/rpmfusion-free-release-$(rpm -E %fedora).noarch.rpm https://mirrors.rpmfusion.org/nonfree/fedora/rpmfusion-nonfree-release-$(rpm -E %fedora).noarch.rpm
        > 
        > # 2. Swap to full ffmpeg
        > sudo dnf swap ffmpeg-free ffmpeg --allowerasing
        > ```
    *   Rust (if building from source).
*   **Source**: A FritzBox Cable router with DVB-C streaming enabled.

## 🛠️ Installation

### From Source (Rust)

```bash
# Clone the repository
git clone https://github.com/dietmar/fritztv.git
cd fritztv

# Build in release mode
cargo build --release

# Run the binary
./target/release/fritztv --config config.toml
```

### Building Packages

Fritztv includes a `Makefile` to automate package creation:

```bash
# Build RPM package (requires rpmbuild)
make rpm

# Build Debian package (uses Docker)
make deb-container

# Build all supported packages (RPM, Deb, Windows/macOS binaries)
make packages
```

The artifacts will be placed in the `dist/` or `rpmbuild/` directories.

## ⚙️ Configuration

Copy the example configuration and adjust it to your needs:

```bash
cp config.toml my_config.toml
```

**`config.toml` Reference:**

```toml
[server]
host = "0.0.0.0"
port = 3000
max_parallel_streams = 4  # Limit concurrent transcode sessions

[fritzbox]
# URL(s) to the M3U playlist extracted from your FritzBox interface
playlist_urls = [
    "https://192.168.178.1/dvb/m3u/tvsd.m3u",
    "https://192.168.178.1/dvb/m3u/tvhd.m3u"
]

[transcoding]
# Mode options:
# - "Smooth": Recommended for best compatibility (Sync correction + buffer safety)
# - "LowLatency": Minimal buffering, faster startup but strictly less tolerant to network jitter
mode = "Smooth"

# Transport options:
# - "udp": Standard (lower latency, may drop packets)
# - "tcp": Reliable (prevents artifacts on bad wifi, slightly higher latency)
transport = "udp"
```

## 🖥️ Usage

### Running Locally

```bash
fritztv --config config.toml
```
Visit `http://localhost:3000` in your browser.

### Systemd Service

An example systemd unit is provided (`fritztv.service`). To install:

1.  Copy binary to `/usr/bin/fritztv`.
2.  Copy config to `/etc/fritztv/config.toml`.
3.  Install service file:
    ```bash
    sudo cp fritztv.service /etc/systemd/system/
    sudo systemctl daemon-reload
    sudo systemctl enable --now fritztv
    ```

## 🏗️ Architecture

Fritztv acts as a proxy and transcoder:
1.  **Ingest**: Connects to FritzBox via RTSP.
2.  **Transcode**: Spawns an `ffmpeg` process to convert MPEG-TS (MPEG-2/H.264) into fMP4 (fragmented MP4) or HLS.
3.  **Serve**: Delivers the stream via HTTP/WebSocket to the client.

It implements a **Universal Sync Fix**:
*   Uses `aresample=async=1` to correct audio timestamp drift (crucial for Chrome/Firefox).
*   Enforces constant frame rate (`-vsync 1`) for stable browser playback.
*   Maintains H.264 `baseline` profile for maximum iOS compatibility.

## 📄 License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
