# Routune Roadmap

## Phase 1 — Foundation (done)

- Cargo workspace (`routune-daemon`, `routune-gui`, `routune-core`)
- gRPC IPC bridge with lock-free telemetry stream
- Tauri GUI with mixer, routing, and DSP tuning views

## Phase 2 — Virtual Audio Routing (current, Linux)

- PipeWire node graph routing and virtual sinks
- Per-application channel assignment via pw-cli
- Node-based DSP architecture validated in user-space

## Phase 3 — DSP Pipeline

- 10-band parametric EQ (`biquad`)
- Asynchronous resampling (`rubato`)
- Noise suppression (`nnnoiseless`)

## Phase 4 — Cross-Platform

- **Windows:** Sysvad-based virtual audio driver (kernel-mode); Test Mode required for MVP distribution
- **macOS:** CoreAudio virtual driver (TBD)
