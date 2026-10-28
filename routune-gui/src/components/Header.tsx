import { Volume2, Wifi, WifiOff, AlertTriangle, RefreshCw } from "lucide-react";
import { useAudioStore } from "../store/useAudioStore";

export function Header() {
  const status = useAudioStore((s) => s.connectionStatus);
  const latency = useAudioStore((s) => s.latencyMs);

  return (
    <header className="flex h-14 items-center justify-between border-b border-neutral-800 bg-neutral-900/60 px-5 backdrop-blur">
      <div className="flex items-center gap-3">
        <div className="flex h-8 w-8 items-center justify-center rounded-lg bg-cyan-500/10 border border-cyan-500/30 text-cyan-400">
          <Volume2 className="h-5 w-5" />
        </div>
        <div>
          <h1 className="text-sm font-semibold tracking-wide text-neutral-100 leading-none">
            Routune
          </h1>
          <span className="text-[10px] text-neutral-400 font-mono">
            Audio Routing & Tuning System
          </span>
        </div>
      </div>

      <div className="flex items-center gap-3">
        {status === "connected" && (
          <div className="flex items-center gap-2 rounded-full border border-emerald-500/30 bg-emerald-500/10 px-3 py-1 text-xs font-medium text-emerald-400">
            <Wifi className="h-3.5 w-3.5" />
            <span>Connected ({latency}ms)</span>
          </div>
        )}

        {status === "connecting" && (
          <div className="flex items-center gap-2 rounded-full border border-amber-500/30 bg-amber-500/10 px-3 py-1 text-xs font-medium text-amber-400">
            <RefreshCw className="h-3.5 w-3.5 animate-spin" />
            <span>Connecting...</span>
          </div>
        )}

        {status === "disconnected" && (
          <div className="flex items-center gap-2 rounded-full border border-neutral-700 bg-neutral-800 px-3 py-1 text-xs font-medium text-neutral-400">
            <WifiOff className="h-3.5 w-3.5" />
            <span>Daemon Offline</span>
          </div>
        )}

        {status === "error" && (
          <div className="flex items-center gap-2 rounded-full border border-red-500/30 bg-red-500/10 px-3 py-1 text-xs font-medium text-red-400">
            <AlertTriangle className="h-3.5 w-3.5" />
            <span>IPC Error</span>
          </div>
        )}
      </div>
    </header>
  );
}
