# Task 03: Linux PipeWire Routing & Node Graph

## Objective

Implement the virtual audio routing backbone for Linux using PipeWire. The daemon must dynamically spawn virtual audio sinks (e.g., "Routune Game", "Routune Chat") and route them to the physical output hardware using a modular, lock-free Node-based graph.

## Cross-Distro Compatibility

The code MUST be distro-agnostic. It must work seamlessly on Arch, Fedora, Ubuntu, and other modern distros.

- Rely entirely on the official `pipewire-rs` bindings and `pkg-config`.
- Do NOT use hardcoded library paths (`/usr/lib`, `/lib64`).
- Do NOT rely on CLI session managers (`pw-cli`, `wpctl`) via `std::process::Command`. Program against the PipeWire core API directly so it works regardless of whether the user has WirePlumber or an older media session manager.

## Execution Steps

1. **Virtual Sink Creation:** Use `pipewire-rs` to programmatically create virtual sinks when the daemon starts, ensuring they register correctly in the OS audio control panel.
2. **Node Graph Architecture:** Update `routune-core` to define a trait/struct system for Audio Nodes (e.g., `VirtualInputNode`, `ResamplerNode`, `PhysicalOutputNode`). The design must be OS-agnostic so the exact same graph logic applies to Windows later.
3. **Stream Bridging:** Capture the raw PCM stream from the PipeWire virtual sinks, pass it through an empty DSP node (bypass for now), and output it to the physical default sink.
4. **Resampling Node:** Integrate the `rubato` crate. If the PipeWire virtual sink runs at a different sample rate than the physical sink, dynamically resample the frames.
5. **IPC Update:** Ensure the active graph topology and volume metrics of these virtual sinks are broadcasted to the Tauri GUI via the existing gRPC stream.

## AI Instructions

Act as a Linux Audio Systems Engineer. Use the Sequential Thinking MCP to design the ownership model of the Node Graph before writing the Rust code. The graph must operate without `Mutex` locks in the audio processing loop. Use atomic queues to handle dynamic linking/unlinking of nodes while the stream is running.
