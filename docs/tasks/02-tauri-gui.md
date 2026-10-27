# Task 02: Tauri GUI & gRPC Client Integration

## Objective

Develop the React frontend in `routune-gui`. The GUI must act strictly as a visual client. It will communicate with the Rust daemon using gRPC (gRPC-Web or standard gRPC via a Node/Rust client sidecar) instead of relying solely on Tauri's native IPC, ensuring the daemon can live on even if the UI is closed.

## Execution Steps

1. **gRPC Client Setup:** Use `@protobuf-ts/plugin` or similar tooling to generate TypeScript client code from the `proto/routune.proto` file created in Task 01.
2. **State Management (Zustand):** Create a Zustand store `useAudioStore`. It should manage connection status and hold the latest telemetry data (volume levels).
3. **Stream Consumption:** Implement a `useEffect` hook in a dedicated hook/service to consume the `StreamTelemetry` gRPC endpoint and update the Zustand store at a high refresh rate.
4. **UI Implementation:** Build a clean, dark-themed UI using TailwindCSS containing a real-time volume meter.

## Performance Constraint

Ensure the React components consuming the real-time volume data do not trigger unnecessary re-renders of the entire app tree. Localize the telemetry state to the specific meter component.
