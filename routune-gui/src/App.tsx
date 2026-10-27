import { Activity, Cpu, Sliders, Volume2 } from "lucide-react";

export function App() {
  return (
    <div className="flex h-screen w-screen flex-col bg-neutral-950 text-neutral-100">
      {/* Top Header */}
      <header className="flex h-12 items-center justify-between border-b border-neutral-800 px-4">
        <div className="flex items-center gap-2">
          <Volume2 className="h-5 w-5 text-cyan-400" />
          <span className="font-semibold tracking-wide text-neutral-100">Routune Control Hub</span>
          <span className="rounded bg-neutral-800 px-2 py-0.5 text-xs text-neutral-400">Phase 1 Scaffold</span>
        </div>
        <div className="flex items-center gap-3 text-xs text-neutral-400">
          <span className="flex items-center gap-1">
            <span className="h-2 w-2 rounded-full bg-emerald-500 animate-pulse" />
            GUI Standalone Mode
          </span>
        </div>
      </header>

      {/* Main Container */}
      <main className="flex flex-1 p-6">
        <div className="grid w-full grid-cols-1 md:grid-cols-3 gap-6">
          {/* Daemon Status Card */}
          <div className="flex flex-col rounded-lg border border-neutral-800 bg-neutral-900/50 p-5">
            <div className="flex items-center gap-2 text-cyan-400">
              <Cpu className="h-5 w-5" />
              <h2 className="font-medium text-neutral-200">Audio Daemon Service</h2>
            </div>
            <p className="mt-2 text-xs text-neutral-400 leading-relaxed">
              Decoupled background process (<code>routune-daemon</code>) handles all WASAPI loopback, lock-free ring buffers, and DSP pipelines.
            </p>
            <div className="mt-auto pt-4 text-xs font-mono text-neutral-500">
              IPC: gRPC :50051 (Planned)
            </div>
          </div>

          {/* Graph Routing Card */}
          <div className="flex flex-col rounded-lg border border-neutral-800 bg-neutral-900/50 p-5">
            <div className="flex items-center gap-2 text-indigo-400">
              <Sliders className="h-5 w-5" />
              <h2 className="font-medium text-neutral-200">Routing Matrix</h2>
            </div>
            <p className="mt-2 text-xs text-neutral-400 leading-relaxed">
              Shared models in <code>routune-core</code> define source/sink nodes and edges. GUI dispatches JSON/Protobuf graph mutations.
            </p>
            <div className="mt-auto pt-4 text-xs font-mono text-neutral-500">
              Audio Routing: No Audio in GUI
            </div>
          </div>

          {/* Telemetry Card */}
          <div className="flex flex-col rounded-lg border border-neutral-800 bg-neutral-900/50 p-5">
            <div className="flex items-center gap-2 text-emerald-400">
              <Activity className="h-5 w-5" />
              <h2 className="font-medium text-neutral-200">Telemetry Stream</h2>
            </div>
            <p className="mt-2 text-xs text-neutral-400 leading-relaxed">
              High-frequency 60 FPS meter and FFT spectrum rendered via Canvas API without audio thread locking.
            </p>
            <div className="mt-auto pt-4 text-xs font-mono text-neutral-500">
              Status: Ready for IPC client
            </div>
          </div>
        </div>
      </main>
    </div>
  );
}

export default App;
