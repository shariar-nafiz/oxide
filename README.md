# Oxide

Oxide is an ultra-fast, lightweight, cross-platform download manager built 100% in Rust. It is engineered as a modern alternative to Internet Download Manager (IDM), combining dynamic multi-connection work-stealing, in-place zero-copy disk writes, and native browser integration across Linux, Windows, and macOS.

## Key Features

- Dynamic Multi-Segment Work-Stealing: Instead of static chunk allocations, Oxide actively monitors connection throughput. When a connection completes its byte range, it automatically splits the largest in-flight segment, keeping network saturation at 100% until completion.
- Zero-Copy Positional File I/O: Files are pre-allocated on disk using native OS APIs (`fallocate` on Linux, `fcntl(F_PREALLOCATE)` on macOS, and file sizing on Windows). Worker threads write directly to final byte offsets, completely eliminating post-download file stitching freezes.
- Minimal Memory Footprint: The background daemon runs in the system tray consuming under 7 MB of RAM with 0% GPU usage. The desktop interface opens on demand in under 15 milliseconds.
- Universal Web Browser Support: Native Messaging Host bridge (`oxide-nmh`) and unified Manifest V3 WebExtension for Google Chrome, Brave, Microsoft Edge, Mozilla Firefox, Zen Browser, Opera, Vivaldi, Chromium, and Safari.
- Video Stream Grabber: Native HLS (`.m3u8`) and DASH (`.mpd`) stream manifest parsing and segment download engine, with an unobtrusive download pill injected on HTML5 video players.
- Site Spider: Batch webpage crawling with domain and file extension filtering.
- BitTorrent and Magnet Support: Magnet URI parsing and integration directly inside the unified manager.

## Architecture

The project is structured as a modular Cargo workspace:

```text
oxide/
├── crates/
│   ├── oxide-core/        Core engine (dynamic work-stealing, rate limiter, redb store)
│   ├── oxide-os/          OS abstraction layer (positional I/O, manifests, native paths)
│   ├── oxide-ipc/         Local socket and named pipe communication
│   ├── oxide-nmh/         Native Messaging Host bridge (<400 KB binary)
│   ├── oxide-stream/      HLS and DASH media stream parser and segment merger
│   ├── oxide-spider/      Webpage asset crawler and link filter
│   ├── oxide-torrent/     Magnet link parser and torrent integration
│   ├── oxide-cli/         Terminal command-line interface
│   └── oxide-gui/         Desktop daemon and minimal egui interface
├── browser-extension/     Unified Manifest V3 extension for Chromium and Firefox
└── .github/workflows/     Cross-platform CI/CD release workflow
```

## Getting Started

### Prerequisites

- Rust 1.75+ (`cargo`, `rustc`)
- On Linux: `libgtk-3-dev`, `libayatana-appindicator3-dev`, `libxdo-dev`

### Installation

Clone the repository and build with Cargo:

```bash
git clone https://github.com/shariar-nafiz/oxide.git
cd oxide
cargo build --release
```

Release binaries will be generated under `target/release/`:
- `oxide-dl`: Desktop daemon and GUI
- `oxide-cli`: Command-line interface
- `oxide-nmh`: Browser Native Messaging Host

## Usage

### Command-Line Interface (`oxide-cli`)

#### 1. Download a File

```bash
cargo run -p oxide-cli -- download "https://example.com/large-archive.zip" --connections 16
```

Options:
- `-o, --output <NAME>`: Custom destination filename.
- `-d, --dir <PATH>`: Custom destination directory (defaults to system Downloads).
- `-c, --connections <NUM>`: Max concurrent connections (default: 16).
- `-l, --limit <BPS>`: Speed limit in bytes per second (0 for unlimited).

#### 2. Auto-Register Browser Integration

Registers the Native Messaging Host for all installed web browsers automatically:

```bash
cargo run -p oxide-cli -- install-browsers
```

Supported browsers:
- Google Chrome
- Brave Browser
- Microsoft Edge
- Chromium
- Mozilla Firefox
- Zen Browser
- Vivaldi
- Opera

#### 3. View Paths and Configuration

```bash
cargo run -p oxide-cli -- info
```

### Desktop Interface (`oxide-dl`)

To launch the background tray daemon and desktop user interface:

```bash
cargo run -p oxide-gui
```

## Browser Extension Setup

1. Open your browser's extension management page:
   - Chromium / Chrome / Brave / Edge: `chrome://extensions/`
   - Firefox / Zen: `about:debugging#/runtime/this-firefox`
2. Enable Developer Mode.
3. Click **Load unpacked** (or **Load Temporary Add-on**) and select the `browser-extension` directory.
4. Ensure you have run `oxide-cli install-browsers` once so the browser can communicate with Oxide.

## Contributing

Contributions are welcome and appreciated. Oxide is free and open source software.

1. Fork the repository.
2. Create your feature branch (`git checkout -b feature/amazing-feature`).
3. Ensure all tests and clippy checks pass:
   ```bash
   cargo clippy --workspace -- -D warnings
   cargo test --workspace
   ```
4. Commit your changes (`git commit -m "feat: add amazing feature"`).
5. Push to the branch (`git push origin feature/amazing-feature`).
6. Open a Pull Request.

## License

This project is licensed under the MIT License. It is free to use, modify, distribute, and contribute to. See the [LICENSE](LICENSE) file for details.
