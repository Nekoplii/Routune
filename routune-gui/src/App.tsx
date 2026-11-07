import { Sidebar } from "./components/Sidebar";
import { Header } from "./components/Header";
import { MixerPanel } from "./components/MixerPanel";
import { RoutingView } from "./components/RoutingView";
import { DspTuningView } from "./components/DspTuningView";
import { Settings } from "./components/Settings";
import { AboutView } from "./components/AboutView";
import { useAudioStore } from "./store/useAudioStore";
import { useTelemetryStream } from "./hooks/useTelemetryStream";
import { useGlobalShortcuts } from "./hooks/useGlobalShortcuts";

export function App() {
  // Start 60FPS gRPC-Web telemetry and heartbeat loop with RAF throttling
  useTelemetryStream();
  // Register system-wide hotkeys from persisted settings
  useGlobalShortcuts();

  const activeView = useAudioStore((s) => s.activeView);

  return (
    <div className="flex h-screen w-screen flex-col bg-[#0F0E17] text-[#ECEAFB] select-none overflow-hidden font-sans">
      {/* Native Multi-Platform Window Titlebar */}
      <Header />

      <div className="flex flex-1 overflow-hidden">
        {/* Studio Navigation Sidebar */}
        <Sidebar />

        {/* Main Studio Viewport */}
        <div className="flex flex-1 flex-col overflow-hidden bg-[#0F0E17]">
          <main className="flex-1 overflow-y-auto p-4 sm:p-6 lg:p-8 w-full max-w-7xl mx-auto">
            {activeView === "mixer" && <MixerPanel />}
            {activeView === "routing" && <RoutingView />}
            {activeView === "tuning" && <DspTuningView />}
            {activeView === "settings" && <Settings />}
            {activeView === "about" && <AboutView />}
          </main>
        </div>
      </div>
    </div>
  );
}

export default App;
