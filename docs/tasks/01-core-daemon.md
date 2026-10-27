# Task 01: Core Daemon & gRPC Foundation

## Objective

Build the foundation of `routune-daemon`. It must run as an independent background service that handles OS audio via `cpal` and exposes a control API via a `tonic` gRPC server.

## Execution Steps

1. **Protobuf Definition:** Create a `proto/routune.proto` file. Define a service `RoutuneControl` with:
    - A standard unary RPC: `Ping (PingRequest) returns (PingResponse)`
    - A server-streaming RPC: `StreamTelemetry (TelemetryRequest) returns (stream AudioTelemetry)` for real-time volume/spectrum data.
2. **gRPC Server (`tonic`):** Implement the server in `routune-daemon`. Bind it to a local loopback address (e.g., `127.0.0.1:50051`).
3. **Audio Capture (`cpal`):** Initialize a `cpal` stream targeting the default output device (acting as a loopback capture).
4. **Lock-Free Bridge:** Set up a lock-free channel (e.g., using `crossbeam-channel` or `rtrb`) between the `cpal` audio thread and the `tonic` async runtime.
5. **DSP Constraint:** Inside the `cpal` data callback, calculate the simple RMS volume of the chunk, push it to the lock-free channel, and do absolutely NO memory allocation (`Vec`, `String`, etc.) or mutex locking.

## AI Instructions

Use the `Sequential Thinking` MCP to map out the lifetimes and thread synchronization between the `cpal` synchronous callback and the `tokio` async runtime before writing the code.
