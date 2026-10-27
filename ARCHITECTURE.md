# Routune: Project Master Architecture

## Project Overview
**Routune** is a cross-platform, low-latency audio routing and DSP tuning application. It acts as an advanced alternative to Sonar SteelSeries, targeting Windows and Linux (and eventually macOS).

## Core Architecture Constraints (Strict Rules for AI)
1. **The Kernel Rule:** Do NOT write or suggest Windows WDM/KS kernel drivers (.sys) during Phase 1. We will use `cpal` for user-mode loopback and standard audio I/O first.
2. **Separation of Concerns:** The GUI (`routune-gui`) is strictly a visual client. It NEVER processes audio. The Background Service (`routune-daemon`) handles ALL audio I/O, DSP, and memory state.
3. **The IPC Bridge:** The GUI and Daemon communicate exclusively via gRPC (or Named Pipes/Unix Sockets). The daemon must survive and continue routing audio even when the GUI is completely closed.
4. **Lock-Free Audio:** The audio processing thread in the daemon must be 100% lock-free and allocation-free. No `Mutex`, `RwLock`, `String`, or `Vec::new()` inside the audio loop. Use lock-free ring buffers (e.g., `rtrb` or `crossbeam` atomic variables) for state updates from the gRPC server.

## Workspace Structure (Monorepo)
```text
routune/
├── Cargo.toml                # Root Workspace config
├── build.rs                  # Build script (protobuf compilation)
├── routune-core/             # Shared Library (DSP, Math, Graph Logic)
├── routune-daemon/           # Background Service (Audio I/O, gRPC Server)
├── routune-gui/              # Tauri + React App (UI, gRPC Client)
├── proto/                    # Protobuf definitions (routune.proto)
└── docs/                     # Documentation and AI Prompts
```

## Tech Stack
- **Daemon/Core:** Rust, `cpal` (Audio I/O), `biquad` (EQ), `rubato` (Resampling), `tonic` (gRPC Server).
- **GUI:** Tauri v2, React, TypeScript, TailwindCSS, Zustand, Canvas API (for rendering 60fps EQ graphs).
- **Driver (Future Phase):** Microsoft MSVAD (Windows) / PipeWire Virtual Sinks (Linux).