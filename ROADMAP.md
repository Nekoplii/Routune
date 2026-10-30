# Routune Development Roadmap (Cross-Platform MVP)

## Phase 1: User-Mode Foundation (Completed)

- Cargo workspace scaffolding (`routune-daemon`, `routune-gui`, `routune-core`).
- Lock-free telemetry and gRPC bridge.
- Tauri GUI functional dashboard.

## Phase 2: Virtual Audio Routing (Current - Linux First)

- **Linux:** Implement PipeWire graph routing and virtual sinks. Validate node-based DSP architecture in user-space.
- **Windows:** Implement Sysvad-based Virtual Audio Driver (Kernel-mode). End-user distribution will require Test Mode for the MVP.
- **Mac:** CoreAudio virtual driver integration (TBD).

## Phase 3: Advanced DSP Pipeline

- Parametric EQ via `biquad`.
- Resampling via `rubato`.
- AI Noise Cancellation (`nnnoiseless`).
