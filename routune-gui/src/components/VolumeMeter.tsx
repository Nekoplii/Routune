import { useEffect, useRef } from "react";
import { useAudioStore } from "../store/useAudioStore";

// Converts dB value (-60dB to 0dB) to a linear 0-100 percentage
function dbToPercent(db: number): number {
  if (db <= -60) return 0;
  if (db >= 0) return 100;
  return ((db + 60) / 60) * 100;
}

export function VolumeMeter() {
  // Subscribe to telemetry using specific selector
  const peakDb = useAudioStore((s) => s.peakDb);
  const rmsDb = useAudioStore((s) => s.rmsDb);
  const isConnected = useAudioStore((s) => s.connectionStatus === "connected");

  const peakHoldRef = useRef<number>(-100);
  const peakHoldDecayTimer = useRef<number>(0);

  // Smooth peak hold decay
  useEffect(() => {
    if (peakDb > peakHoldRef.current) {
      peakHoldRef.current = peakDb;
      peakHoldDecayTimer.current = Date.now();
    } else if (Date.now() - peakHoldDecayTimer.current > 1200) {
      peakHoldRef.current = Math.max(-100, peakHoldRef.current - 0.5);
    }
  }, [peakDb]);

  const peakPct = isConnected ? dbToPercent(peakDb) : 0;
  const rmsPct = isConnected ? dbToPercent(rmsDb) : 0;
  const peakHoldPct = isConnected ? dbToPercent(peakHoldRef.current) : 0;
  const isClipping = peakDb >= -0.1;

  return (
    <div className="flex flex-col gap-4 rounded-xl border border-neutral-800 bg-neutral-900/70 p-5 backdrop-blur shadow-lg">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <span className="text-sm font-semibold tracking-wider uppercase text-neutral-300">
            Master Audio Meter
          </span>
          {isClipping && (
            <span className="animate-pulse rounded bg-red-600/30 px-1.5 py-0.5 text-[10px] font-bold text-red-400 border border-red-500/40">
              CLIP
            </span>
          )}
        </div>
        <div className="flex items-center gap-3 font-mono text-xs text-neutral-400">
          <span>
            PEAK: <strong className={isClipping ? "text-red-400" : "text-neutral-200"}>
              {isConnected ? `${peakDb.toFixed(1)} dB` : "-∞ dB"}
            </strong>
          </span>
          <span>
            RMS: <strong className="text-neutral-200">
              {isConnected ? `${rmsDb.toFixed(1)} dB` : "-∞ dB"}
            </strong>
          </span>
        </div>
      </div>

      {/* Main Meter Track */}
      <div className="relative flex flex-col gap-2">
        {/* dB Scale Legend */}
        <div className="flex justify-between px-1 text-[10px] font-mono text-neutral-500 select-none">
          <span>-60</span>
          <span>-48</span>
          <span>-36</span>
          <span>-24</span>
          <span>-18</span>
          <span>-12</span>
          <span>-6</span>
          <span>-3</span>
          <span className="text-red-400 font-semibold">0</span>
        </div>

        {/* Peak Bar */}
        <div className="relative h-6 w-full overflow-hidden rounded bg-neutral-950 border border-neutral-800 p-0.5">
          {/* Background gradient zones */}
          <div
            className="h-full rounded-sm transition-all duration-75 ease-out"
            style={{
              width: `${peakPct}%`,
              background:
                "linear-gradient(90deg, #10b981 0%, #10b981 70%, #f59e0b 85%, #ef4444 100%)",
            }}
          />

          {/* Peak Hold Marker */}
          {peakHoldPct > 0 && (
            <div
              className="absolute top-0 bottom-0 w-0.5 bg-white shadow-sm transition-all duration-100 pointer-events-none"
              style={{ left: `${Math.min(peakHoldPct, 99.5)}%` }}
            />
          )}
        </div>

        {/* RMS Bar (Subtle) */}
        <div className="relative h-2 w-full overflow-hidden rounded bg-neutral-950 border border-neutral-800/80 p-[1px]">
          <div
            className="h-full rounded-sm bg-cyan-400/80 transition-all duration-100 ease-out"
            style={{ width: `${rmsPct}%` }}
          />
        </div>
      </div>

      <div className="flex items-center justify-between text-[11px] text-neutral-500 font-mono">
        <span>Channel: System Loopback (Master)</span>
        <span>Sampling: 60 FPS Telemetry</span>
      </div>
    </div>
  );
}
