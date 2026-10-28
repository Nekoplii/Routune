import { Header } from "./components/Header";
import { VolumeMeter } from "./components/VolumeMeter";
import { DaemonStatusCard } from "./components/DaemonStatusCard";
import { useTelemetryStream } from "./hooks/useTelemetryStream";
import { Sliders, Activity } from "lucide-react";

export function App() {
  // Start gRPC-Web telemetry and heartbeat loop
  useTelemetryStream();

  return (
    <div className="flex h-screen w-screen flex-col bg-neutral-950 text-neutral-100 select-none overflow-hidden font-sans">
      <Header />

      <main className="flex-1 overflow-y-auto p-6 space-y-6 max-w-6xl w-full mx-auto">
        {/* Real-time Meter (High-Frequency Localized Render) */}
        <section>
          <VolumeMeter />
        </section>

        {/* System & Architecture Grid */}
        <section className="grid grid-cols-1 md:grid-cols-3 gap-5">
          <DaemonStatusCard />

          {/* Routing Matrix Card */}
          <div className="flex flex-col rounded-xl border border-neutral-800 bg-neutral-900/40 p-5">
            <div className="flex items-center gap-2 text-indigo-400">
              <Sliders className="h-5 w-5" />
              <h2 className="text-sm font-medium text-neutral-200">Routing Topology</h2>
            </div>
            <p className="mt-2 text-xs text-neutral-400 leading-relaxed">
              Shared definitions in <code className="text-neutral-300">routune-core</code> define source/sink node channels. The GUI mutates graphs via IPC commands.
            </p>
            <div className="mt-auto pt-4 flex items-center justify-between text-xs font-mono text-neutral-500">
              <span>Graph Engine:</span>
              <span className="text-indigo-400">Phase 2 Target</span>
            </div>
          </div>

          {/* DSP Processing Card */}
          <div className="flex flex-col rounded-xl border border-neutral-800 bg-neutral-900/40 p-5">
            <div className="flex items-center gap-2 text-emerald-400">
              <Activity className="h-5 w-5" />
              <h2 className="text-sm font-medium text-neutral-200">DSP Tuning Pipeline</h2>
            </div>
            <p className="mt-2 text-xs text-neutral-400 leading-relaxed">
              Parametric EQ (<code className="text-neutral-300">biquad</code>) and sample-rate conversion (<code className="text-neutral-300">rubato</code>) operate entirely in daemon user-mode.
            </p>
            <div className="mt-auto pt-4 flex items-center justify-between text-xs font-mono text-neutral-500">
              <span>Filter Precision:</span>
              <span className="text-emerald-400">32-bit Float</span>
            </div>
          </div>
        </section>
      </main>
    </div>
  );
}

export default App;
