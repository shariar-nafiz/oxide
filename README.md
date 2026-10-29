# Oxide (`oxide-dl`)

> A blazing-fast, ultra-lightweight, 100% Rust cross-platform download manager and direct competitor to Internet Download Manager (IDM).

## Highlights
- **Zero-GUI-RAM Background Daemon:** Runs at **< 4–7 MB RAM** in the system tray with **0% GPU/VRAM**. Spawns the clean minimal interface on demand in < 15 ms.
- **True Cross-Platform:** 100% native support for **Linux** (X11 & Wayland), **Windows** (10 & 11), and **macOS** (Intel & Apple Silicon).
- **Dynamic Segment Work-Stealing:** IDM's core advantage rebuilt in modern async Rust with in-flight chunk splitting and work-stealing so connections remain 100% saturated.
- **Zero-Copy Positional File I/O:** Pre-allocates files on disk and writes directly to final offsets—no slow end-of-download file stitching/rebuilding freezes!
- **All-Browser Integration:** Unified Manifest V3 WebExtension + Native Messaging Host for Chrome, Brave, Edge, Firefox, Zen, Opera, Vivaldi, and Safari.
- **Stream Grabber & Media Sniffer:** Built-in HLS (`.m3u8`) and DASH (`.mpd`) video stream extraction and segment muxing.
- **Site Spider & BitTorrent:** Batch webpage crawling and native Magnet/torrent downloading in a single interface.

## Workspace Crates
- `crates/oxide-os`: Cross-platform OS abstraction layer (positional I/O, browser manifests, power/sleep control).
- `crates/oxide-core`: Core download engine (dynamic work-stealing, rate limiter, queue scheduler, `redb` persistence).
- `crates/oxide-ipc`: Local IPC layer (Unix Domain Sockets on Linux/macOS + Named Pipes on Windows).
- `crates/oxide-nmh`: Ultra-tiny (<400 KB) Native Messaging Host bridge for web browsers.
- `crates/oxide-stream`: HLS & DASH manifest parser, segment grabber, and container muxer.
- `crates/oxide-spider`: Webpage link crawler and asset filter.
- `crates/oxide-torrent`: BitTorrent & Magnet engine.
- `crates/oxide-cli`: Terminal client (`oxide-cli`).
- `crates/oxide-gui`: Background tray daemon + on-demand minimal `egui` (`eframe`/`glow`) UI (`oxide-dl`).
- `browser-extension`: Unified Manifest V3 extension for Chromium and Firefox/Zen families.

## Documentation
- [System Architecture](file:///home/shariarunix/Desktop/unix-downloader/docs/ARCHITECTURE.md)
- [Implementation Roadmap](file:///home/shariarunix/Desktop/unix-downloader/docs/ROADMAP.md)

## License
Licensed under MIT or Apache-2.0.
