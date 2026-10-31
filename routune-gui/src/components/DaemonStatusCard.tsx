import { Cpu, CheckCircle2, XCircle } from "lucide-react";
import { useAudioStore } from "../store/useAudioStore";

export function DaemonStatusCard() {
  const isAudioActive = useAudioStore((s) => s.isAudioActive);
  const status = useAudioStore((s) => s.connectionStatus);
  const lastError = useAudioStore((s) => s.lastError);
  const channelMetrics = useAudioStore((s) => s.channelMetrics);

  return (
    <div className="flex flex-col rounded-xl border border-neutral-800 bg-neutral-900/40 p-5">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2 text-cyan-400">
          <Cpu className="h-5 w-5" />
          <h2 className="text-sm font-medium text-neutral-200">Daemon Engine Status</h2>
        </div>
        {status === "connected" ? (
          <span className="flex items-center gap-1 text-xs text-emerald-400">
            <CheckCircle2 className="h-3.5 w-3.5" />
            Online
          </span>
        ) : (
          <span className="flex items-center gap-1 text-xs text-neutral-500">
            <XCircle className="h-3.5 w-3.5" />
            Unavailable
          </span>
        )}
      </div>

      <div className="mt-4 space-y-2 text-xs">
        <div className="flex justify-between border-b border-neutral-800/60 pb-1.5">
          <span className="text-neutral-400">Transport:</span>
          <span className="font-mono text-neutral-200">gRPC-Web (:50051)</span>
        </div>
        <div className="flex justify-between border-b border-neutral-800/60 pb-1.5">
          <span className="text-neutral-400">Audio Loopback:</span>
          <span
            className={`font-mono ${
              isAudioActive ? "text-emerald-400" : "text-amber-400"
            }`}
          >
            {isAudioActive ? "PipeWire / Audio Active" : "Idle / No Stream"}
          </span>
        </div>
        <div className="flex justify-between border-b border-neutral-800/60 pb-1.5">
          <span className="text-neutral-400">Audio Thread:</span>
          <span className="font-mono text-neutral-200">Lock-Free RingBuffer</span>
        </div>
        <div className="flex justify-between pb-1.5">
          <span className="text-neutral-400">GUI Architecture:</span>
          <span className="font-mono text-cyan-400">Pure Visual Client</span>
        </div>
      </div>

      {lastError && (
        <div className="mt-3 rounded border border-red-500/20 bg-red-500/10 p-2 text-[11px] text-red-400 font-mono">
          {lastError}
        </div>
      )}

      {/* 5-Channel Telemetry IPC Dump */}
      <div className="mt-4 rounded-lg border border-neutral-800 bg-neutral-950/80 p-3">
        <div className="flex items-center justify-between border-b border-neutral-800 pb-1.5 text-[11px] font-semibold text-neutral-400 uppercase tracking-wider">
          <span>5-Channel Telemetry</span>
          <span className="font-mono text-cyan-400">
            {channelMetrics.length > 0 ? `${channelMetrics.length} Channels` : "Idle"}
          </span>
        </div>
        <div className="mt-2 space-y-1 font-mono text-[11px]">
          {channelMetrics.length === 0 ? (
            <div className="text-neutral-600 italic">Waiting for telemetry stream...</div>
          ) : (
            channelMetrics.map((ch) => (
              <div
                key={ch.channelName || ch.channel}
                className="flex justify-between items-center text-neutral-300"
              >
                <span className="text-neutral-400">{ch.channelName}:</span>
                <span className="text-emerald-400">
                  Peak: {ch.peakVolumeDb.toFixed(1)} dB
                </span>
                <span className="text-cyan-400">
                  RMS: {ch.rmsVolumeDb.toFixed(1)} dB
                </span>
              </div>
            ))
          )}
        </div>
        {channelMetrics.length > 0 && (
          <details className="mt-2 text-[10px] text-neutral-500 font-mono">
            <summary className="cursor-pointer hover:text-neutral-400">Raw IPC JSON</summary>
            <pre className="mt-1 max-h-32 overflow-auto text-neutral-400 bg-neutral-900/60 p-1.5 rounded">
              {JSON.stringify(channelMetrics, null, 2)}
            </pre>
          </details>
        )}
      </div>
    </div>
  );
}
