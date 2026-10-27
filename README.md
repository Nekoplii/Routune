# Routune

Low-latency background audio routing and DSP tuning daemon with decoupled control GUI.

## Architecture

Routune uses a decoupled client-server architecture:
- **`routune-daemon`**: Background system service. Manages OS audio devices via user-mode loopback (`cpal`), runs lock-free DSP pipelines (`biquad`, `rubato`), and exposes a gRPC IPC control server (`tonic`).
- **`routune-gui`**: Frontend client built with Tauri v2, React, TypeScript, and TailwindCSS. Communicates exclusively over IPC; processes zero audio directly.
- **`routune-core`**: Shared library defining graph topologies, channel configurations, filter definitions, and stream telemetry models.
- **`proto`**: Protocol Buffer specifications defining the IPC API contract.

```text
routune/
├── Cargo.toml                # Workspace definition
├── proto/                    # Protobuf service definitions
│   └── routune.proto
├── routune-core/             # Shared DSP/Graph structs
├── routune-daemon/           # Background audio service
└── routune-gui/              # Tauri v2 + React GUI
```

## Tech Stack

- **Audio Engine:** Rust (Edition 2024), `cpal`, `biquad`, `rubato`, `tokio`
- **IPC Interface:** gRPC (`tonic`, `protobuf-ts`)
- **Frontend Client:** Tauri v2, React 18, Vite, TypeScript, TailwindCSS, Zustand

## Prerequisites

- **Rust:** `rustc` and `cargo` (1.85+ recommended)
- **Node.js:** Node 18+ and `npm`
- **Platform Dependencies:**
  - Windows: Microsoft C++ Build Tools and Windows SDK

## Building

### Cargo Workspace

Compile all Rust crates (`routune-core`, `routune-daemon`, `routune-gui` Tauri shell):

```bash
cargo build --workspace
```

Run workspace unit tests:

```bash
cargo test --workspace
```

### GUI Frontend

Install dependencies and compile web assets:

```bash
cd routune-gui
npm install
npm run build
```

Run Tauri application in development mode:

```bash
cd routune-gui
npm run tauri dev
```

## Roadmap

See [ROADMAP.md](./ROADMAP.md) for milestone progress and [ARCHITECTURE.md](./ARCHITECTURE.md) for architectural constraints.

## License

This project is licensed under the GNU General Public License v3.0 only ([LICENSE](./LICENSE)).
