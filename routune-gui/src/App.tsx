import { Sidebar } from "./components/Sidebar";
import { Header } from "./components/Header";
import { MixerPanel } from "./components/MixerPanel";
import { RoutingView } from "./components/RoutingView";
import { DspTuningView } from "./components/DspTuningView";
import { AboutView } from "./components/AboutView";
import { useAudioStore } from "./store/useAudioStore";
import { useTelemetryStream } from "./hooks/useTelemetryStream";

export function App() {
  // Start 60FPS gRPC-Web telemetry and heartbeat loop with RAF throttling
  useTelemetryStream();

  const activeView = useAudioStore((s) => s.activeView);

  return (
    <div className="flex h-screen w-screen flex-col bg-[#131316] text-zinc-100 select-none overflow-hidden font-sans">
      {/* Native Multi-Platform Window Titlebar */}
      <Header />

      <div className="flex flex-1 overflow-hidden">
        {/* Studio Navigation Sidebar */}
        <Sidebar />

        {/* Main Studio Viewport */}
        <div className="flex flex-1 flex-col overflow-hidden bg-[#18181d]">
          <main className="flex-1 overflow-y-auto p-4 sm:p-6 lg:p-8 w-full max-w-7xl mx-auto">
            {activeView === "mixer" && <MixerPanel />}
            {activeView === "routing" && <RoutingView />}
            {activeView === "tuning" && <DspTuningView />}
            {activeView === "about" && <AboutView />}
          </main>
        </div>
      </div>
    </div>
  );
}

export default App;
