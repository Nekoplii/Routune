# Routune

Audio routing and DSP daemon for Linux. The daemon manages all hardware I/O and runs DSP on real-time threads. The Tauri GUI is a control client that communicates over gRPC IPC and can be closed without interrupting routing.

---

## Features

- **Virtual audio routing** — PipeWire node graph with per-application channel assignment
- **10-band parametric EQ** — biquad filter pipeline, lock-free DSP on real-time threads
- **Noise suppression** — RNNoise via `nnnoiseless` on the microphone channel
- **Spatial audio** — stereo widening on output channels
- **Real-time telemetry** — peak/RMS metering and latency counters streamed to GUI
- **Persistent configuration** — EQ profiles and settings stored locally, exportable as JSON
- **Global shortcuts** — system-wide hotkeys for mute and noise toggle
- **Autostart** — daemon launch on desktop session login via systemd user unit

---

## Architecture

```text
routune/
├── Cargo.toml              # Workspace root
├── proto/                  # Protobuf IPC contract
│   └── routune.proto
├── routune-core/           # Shared DSP structs and graph definitions
├── routune-daemon/         # Headless audio service (cpal + PipeWire + tonic)
└── routune-gui/            # Tauri v2 + React 18 frontend
    └── src-tauri/          # Tauri shell and plugin registration
```

**routune-daemon** binds audio streams via `cpal`, hooks into the PipeWire node graph, runs DSP on high-priority real-time threads, and serves a gRPC control API on `127.0.0.1:50051`.

**routune-gui** is a control client. It issues unary RPCs and receives streaming telemetry. Closing it does not affect audio routing.

See [ARCHITECTURE.md](./ARCHITECTURE.md) for detailed Mermaid topology diagrams and IPC specifications.

---

## Platform Status

| Platform                         | Status            |
| -------------------------------- | ----------------- |
| Linux (x86_64, PipeWire)         | ✅ Primary target |
| Windows (Sysvad virtual driver)  | 🚧 Planned        |
| macOS (CoreAudio virtual driver) | 🚧 Planned        |

---

## Prerequisites

### Linux

| Requirement                | Minimum version                |
| -------------------------- | ------------------------------ |
| Rust + Cargo               | 1.85+                          |
| Node.js + npm              | Node 18+                       |
| PipeWire                   | 0.3+ (running in user session) |
| `libwebkit2gtk-4.1`        | Required for Tauri GUI         |
| `libayatana-appindicator3` | Required for Tauri GUI tray    |
| `protobuf-compiler`        | Required for `tonic-build`     |
| `libclang-dev`             | Required for bindgen           |

Install system dependencies on Ubuntu/Debian:

```bash
sudo apt-get install -y \
  libwebkit2gtk-4.1-dev \
  libayatana-appindicator3-dev \
  libgtk-3-dev \
  libssl-dev \
  libasound2-dev \
  libpipewire-0.3-dev \
  libclang-dev \
  protobuf-compiler
```

Install system dependencies on Fedora/RHEL:

```bash
sudo dnf install -y \
  webkit2gtk4.1-devel \
  libayatana-appindicator-gtk3-devel \
  gtk3-devel \
  openssl-devel \
  alsa-lib-devel \
  pipewire-devel \
  clang-devel \
  protobuf-compiler
```

---

## Building from Source

### 1. Clone the repository

```bash
git clone https://github.com/routune/routune.git
cd routune
```

### 2. Build the daemon

```bash
cargo build --release -p routune-daemon
```

Binary output: `target/release/routune-daemon`

### 3. Build and run the GUI

```bash
cd routune-gui
npm install
npm run tauri dev        # development with hot-reload
```

Build a production GUI bundle (`.deb`, `.rpm`, `.AppImage`):

```bash
npm run tauri build
```

Bundle output: `routune-gui/src-tauri/target/release/bundle/`

### 4. Run workspace tests

```bash
cargo test --workspace
```

---

## Installation

### One-Line Quick Install (Recommended)

Install Routune daemon, GUI, systemd background service, desktop launcher, and icons:

```bash
curl -fsSL https://raw.githubusercontent.com/mafly/routune/main/install.sh | bash
```

Or install system-wide (requires sudo):
```bash
curl -fsSL https://raw.githubusercontent.com/mafly/routune/main/install.sh | bash -s -- --system
```

To uninstall at any time:
```bash
./install.sh --uninstall
```

### Option B — Pre-built packages

Download the latest release assets from the [Releases](../../releases) page:

| Package              | Format          | Install command                                       |
| -------------------- | --------------- | ----------------------------------------------------- |
| `routune_*.deb`      | Debian/Ubuntu   | `sudo dpkg -i routune_*.deb`                          |
| `routune-*.rpm`      | Fedora/RHEL     | `sudo rpm -i routune-*.rpm`                           |
| `routune_*.AppImage` | Universal Linux | `chmod +x routune_*.AppImage && ./routune_*.AppImage` |
| `routune-daemon`     | Raw binary      | Copy to `/usr/local/bin/`                             |

### Option B — Systemd user service (daemon only, headless)

Run the daemon as a background user service that starts with your desktop session:

```bash
# 1. Copy the daemon binary
sudo cp target/release/routune-daemon /usr/bin/routune-daemon

# 2. Install the service unit
mkdir -p ~/.config/systemd/user
cp packaging/routune.service ~/.config/systemd/user/

# 3. Enable and start
systemctl --user daemon-reload
systemctl --user enable --now routune.service

# 4. Verify
systemctl --user status routune.service
journalctl --user -u routune.service -f
```

The daemon listens on `127.0.0.1:50051`. Launch `routune-gui` at any time to connect to it.

---

## Tech Stack

| Layer            | Technology                                       |
| ---------------- | ------------------------------------------------ |
| Audio I/O        | `cpal`                                           |
| Audio graph      | PipeWire (Linux)                                 |
| DSP filters      | `biquad`, `rubato`, `nnnoiseless`                |
| Lock-free queues | `rtrb`, `ringbuf`                                |
| Async runtime    | `tokio`                                          |
| IPC protocol     | gRPC (`tonic`, `protobuf-ts`)                    |
| GUI shell        | Tauri v2                                         |
| Frontend         | React 18, TypeScript, Vite, TailwindCSS, Zustand |

---

## Roadmap

See [ROADMAP.md](./ROADMAP.md) for milestone progress.

---

## License

GNU General Public License v3.0 only — see [LICENSE](./LICENSE).
