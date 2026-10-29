# Oxide Architecture Specification

## 1. High-Level System Architecture

Oxide uses a decoupled client-daemon architecture across Linux, Windows, and macOS:

```
+-------------------------------------------------------------------+
|                        Web Browsers                               |
| (Chrome, Brave, Edge, Firefox, Zen, Opera, Vivaldi, Safari)       |
+-------------------------------------------------------------------+
                                  |
                                  | Native Messaging (stdio framed JSON)
                                  v
+-------------------------------------------------------------------+
|               oxide-nmh (Native Messaging Host, <400KB)           |
+-------------------------------------------------------------------+
                                  |
                                  | Local IPC (Unix Socket / Win Named Pipe)
                                  v
+-------------------------------------------------------------------+
|                oxide-dl Background Daemon (~4–6 MB RAM)           |
|                                                                   |
|  - oxide-os: Cross-platform disk preallocation & positional I/O  |
|  - oxide-core: Dynamic work-stealing download engine + redb       |
|  - oxide-stream: HLS / DASH media stream grabber                  |
|  - oxide-spider: HTML crawler & batch asset extractor             |
|  - oxide-torrent: Magnet & BitTorrent engine                      |
|  - System Tray / Menu Bar Controller (tray-icon)                  |
+-------------------------------------------------------------------+
             |                                        |
             | IPC / In-Memory Channel                | Spawns On-Demand
             v                                        v
+-------------------------+              +--------------------------+
|  oxide-cli (Terminal)   |              |  oxide-gui (Minimal UI)  |
|                         |              |  - egui + glow           |
|                         |              |  - 0% CPU when idle      |
|                         |              |  - Destroyed when closed |
+-------------------------+              +--------------------------+
```

## 2. Dynamic Segment Work-Stealing Algorithm

Unlike naive downloaders that partition a file into fixed slices at the start:
1. Oxide probes headers (`Accept-Ranges: bytes`, `Content-Length`, `ETag`, `Content-Disposition`).
2. Scales from 1 to $N$ concurrent workers dynamically.
3. When worker $W_i$ finishes its range $[S_i, E_i]$, the coordinator finds the busy worker $W_b$ with the largest remaining bytes $R = E_b - C_b$.
4. If $R \ge 2 \times \text{MIN\_SPLIT\_SIZE}$ (512 KB), $W_b$'s end offset is atomically clamped to $M = \lfloor(C_b + E_b)/2\rfloor$, and $W_i$ takes $[M+1, E_b]$.
5. File writes bypass chunk buffers by writing directly to final file offsets using:
   - Linux: `std::os::unix::fs::FileExt::write_at` (`pwrite64`)
   - macOS: `std::os::unix::fs::FileExt::write_at` (`pwrite`)
   - Windows: `std::os::windows::fs::FileExt::seek_write` (`OVERLAPPED` `WriteFile`)

## 3. Zero-GUI-RAM Background Lifecycle
To keep idle RAM under 7 MB:
- The daemon loop runs with only Tokio and `tray-icon`.
- When the window is closed to the system tray, the window viewport, event loop, and OpenGL context are completely deallocated.
- On macOS, `NSApplicationActivationPolicyAccessory` hides the Dock icon.
- On Windows, `SetProcessWorkingSetSize` trims idle memory pages.

## 4. Browser Integration & Manifests
The `oxide-os` module automatically installs Native Messaging manifests:
- **Linux:** `~/.config/<browser>/NativeMessagingHosts/org.oxide.nmh.json` and `~/.mozilla/native-messaging-hosts/org.oxide.nmh.json`.
- **macOS:** `~/Library/Application Support/<Browser>/NativeMessagingHosts/org.oxide.nmh.json`.
- **Windows:** Registry keys under `HKCU\Software\<Browser>\NativeMessagingHosts\org.oxide.nmh` pointing to the JSON manifest in `%APPDATA%\Oxide\`.
