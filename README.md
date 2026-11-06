# Routune

Audio routing and DSP daemon for Linux. The daemon manages all hardware I/O and runs DSP on real-time threads. The Tauri GUI is a control client that communicates over gRPC IPC and can be closed without interrupting routing.

---

## Features

- **Virtual audio routing** — PipeWire node graph with per-application channel assignment (Game, Chat, Media, Aux, Mic)
- **Virtual microphone pipeline** — Dedicated native PipeWire virtual source (`routune-virtual-mic`) ready for Discord, OBS, and games with zero feedback loops
- **Full microphone DSP suite**:
    - **AI noise suppression** — RNNoise via `nnnoiseless` with real-time attenuation level control
    - **Noise gate** — Configurable threshold, attack, and release to eliminate background room noise
    - **10-band parametric EQ** — Peaking biquad filter pipeline per channel
    - **Dynamic range compressor** — Threshold, ratio, attack, release, and makeup gain
    - **Mic test / monitor toggle** — Real-time loopback to headphones for audio tuning without echo
- **Spatial audio & volume boost** — Stereo widening and soft-clipped gain amplification on output channels
- **Lock-free real-time DSP** — Zero-allocation, wait-free ring buffers on high-priority audio threads
- **Resilient recovery supervisor** — Auto-recovery loop with backoff and automatic sound server fallback (`pulse`/`pipewire`) on hardware disconnects
- **Real-time telemetry** — High-frequency peak/RMS volume metering and latency statistics streamed via gRPC
- **Persistent configuration** — Routes, EQ profiles, and dynamics stored locally in `~/.config/routune/config.json`
- **Global shortcuts** — System-wide hotkeys for mute toggles and noise cancellation
- **Desktop integration & autostart** — Systemd user unit, desktop launcher, tray indicator, and one-line universal installer

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

Install system dependencies on Arch Linux:

```bash
sudo pacman -S --needed \
  webkit2gtk-4.1 \
  libayatana-appindicator \
  gtk3 \
  openssl \
  alsa-lib \
  pipewire \
  clang \
  protobuf
```

---

## Building from Source

### 1. Clone the repository

```bash
git clone https://github.com/mafly/routune.git
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
curl -fsSL https://raw.githubusercontent.com/Nekoplii/Routune/main/install.sh | bash
```

Or install system-wide (requires sudo):

```bash
curl -fsSL https://raw.githubusercontent.com/Nekoplii/Routune/main/install.sh | bash -s -- --system
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

---

## Tech Stack

| Layer            | Technology                                                     |
| ---------------- | -------------------------------------------------------------- |
| Audio I/O        | `cpal` (ALSA / Pulse / PipeWire streaming)                     |
| Audio graph      | PipeWire (`pipewire-rs`, `pw-link`, native virtual mic)        |
| DSP filters      | `biquad` (EQ), `nnnoiseless` (RNNoise), noise gate, compressor |
| Lock-free queues | `crossbeam-channel`, atomic `AudioRingBuffer`                  |
| Async runtime    | `tokio`                                                        |
| IPC protocol     | gRPC (`tonic`, `prost`, `@protobuf-ts`)                        |
| GUI shell        | Tauri v2                                                       |
| Frontend         | React 18, TypeScript, Vite, TailwindCSS, Zustand               |

---

## Roadmap

See [ROADMAP.md](./ROADMAP.md) for milestone progress.

---

## License

GNU General Public License v3.0 only — see [LICENSE](./LICENSE).
