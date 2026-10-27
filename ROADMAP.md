# Routune Development Roadmap

## Phase 1: User-Mode MVP (Current)

- Scaffolding Cargo workspace (`routune-daemon`, `routune-gui`, `routune-core`).
- Basic Tauri v2 + React setup.
- Rust daemon boilerplate with gRPC server (`tonic`).
- Audio capture using `cpal` (WASAPI loopback on Windows).
- Basic lock-free telemetry (sending volume/spectrum data from daemon to GUI).

## Phase 2: Virtual Audio Routing

- Implementing Virtual Audio Device (Sysvad based) for Windows.
- PipeWire graph routing for Linux.
- Tying virtual endpoints to the DSP pipeline.

## Phase 3: Advanced DSP

- Parametric EQ implementation using `biquad`.
- AI Noise Cancellation (`nnnoiseless`).
- Resampling integration (`rubato`).
