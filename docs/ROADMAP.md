# Oxide Roadmap & Milestones

## Phase 1: Foundation & Dynamic Work-Stealing Core
- [x] Workspace setup & size/RAM compiler profiles
- [x] Initial documentation (`ARCHITECTURE.md`, `ROADMAP.md`, `README.md`)
- [ ] `oxide-os`: Cross-platform preallocation (`fallocate`, `F_PREALLOCATE`, `SetEndOfFile`) and positional I/O (`write_at`, `seek_write`)
- [ ] `oxide-core`: Dynamic HTTP probing, in-flight work-stealing algorithm, rate limiter, and `redb` state database
- [ ] `oxide-cli`: Terminal download client with multi-segment visual progress

## Phase 2: Background Daemon & On-Demand Minimal GUI
- [ ] Cross-platform System Tray / Menu Bar (`tray-icon`)
- [ ] On-demand viewport lifecycle (destroying GL context on close to tray for < 7 MB idle RAM)
- [ ] Minimal modern UI (`egui`/`glow`): Filter pills, micro segment bars, collapsible inspector drawer
- [ ] Compact Spotlight-style intercept card

## Phase 3: All-Browser WebExtension & Native Messaging
- [ ] `oxide-ipc`: Cross-platform local IPC (Unix sockets + Windows Named Pipes)
- [ ] `oxide-nmh`: Stdio length-prefixed JSON bridge for browsers
- [ ] Automated browser manifest / registry installer (`oxide-cli install-browsers`)
- [ ] Unified Manifest V3 WebExtension (Chromium & Firefox/Zen) with cookie/referer mirror and link refresh

## Phase 4: Media Stream Sniffer & Grabber
- [ ] HLS (`.m3u8`) and DASH (`.mpd`) playlist parser & segment downloader in `oxide-stream`
- [ ] Floating video download pill in browser content script
- [ ] Multi-thread segment assembler & container muxer

## Phase 5: Advanced Features & Multi-OS CI
- [ ] `oxide-spider`: HTML link spider with regex and MIME filters
- [ ] `oxide-torrent`: BitTorrent & Magnet engine
- [ ] Cross-platform packaging (.deb, AppImage, MSI, DMG) and CI pipeline
