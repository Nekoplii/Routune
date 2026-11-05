import { useEffect, useRef } from "react";
import { useAudioStore } from "../store/useAudioStore";

function dbToPercent(db: number): number {
  if (db <= -60) return 0;
  if (db >= 0) return 100;
  return ((db + 60) / 60) * 100;
}

export function VolumeMeter() {
  const peakBarRef   = useRef<HTMLDivElement>(null);
  const rmsBarRef    = useRef<HTMLDivElement>(null);
  const holdMarkerRef = useRef<HTMLDivElement>(null);
  const peakLabelRef = useRef<HTMLSpanElement>(null);
  const rmsLabelRef  = useRef<HTMLSpanElement>(null);
  const clipBadgeRef = useRef<HTMLSpanElement>(null);

  const peakHoldRef      = useRef(-100);
  const peakHoldTimerRef = useRef(0);
  const rafRef           = useRef<number | null>(null);

  useEffect(() => {
    function loop() {
      rafRef.current = requestAnimationFrame(loop);

      const state = useAudioStore.getState();
      const isConnected = state.connectionStatus === "connected";
      const peakDb = isConnected ? state.peakDb : -100;
      const rmsDb  = isConnected ? state.rmsDb  : -100;

      // Peak hold
      const now = performance.now();
      if (peakDb >= peakHoldRef.current) {
        peakHoldRef.current = peakDb;
        peakHoldTimerRef.current = now;
      } else if (now - peakHoldTimerRef.current > 1200) {
        peakHoldRef.current = Math.max(-100, peakHoldRef.current - 0.5);
      }

      const peakPct = isConnected ? dbToPercent(peakDb) : 0;
      const rmsPct  = isConnected ? dbToPercent(rmsDb)  : 0;
      const holdPct = isConnected ? dbToPercent(peakHoldRef.current) : 0;
      const isClipping = peakDb >= -0.1;

      if (peakBarRef.current)   peakBarRef.current.style.width   = `${peakPct}%`;
      if (rmsBarRef.current)    rmsBarRef.current.style.width    = `${rmsPct}%`;
      if (holdMarkerRef.current) {
        holdMarkerRef.current.style.display = holdPct > 0 ? "block" : "none";
        holdMarkerRef.current.style.left    = `${Math.min(holdPct, 99.5)}%`;
      }
      if (peakLabelRef.current)
        peakLabelRef.current.textContent = isConnected ? `${peakDb.toFixed(1)} dB` : "-∞ dB";
      if (rmsLabelRef.current)
        rmsLabelRef.current.textContent  = isConnected ? `${rmsDb.toFixed(1)} dB`  : "-∞ dB";
      if (peakLabelRef.current)
        peakLabelRef.current.className = isClipping ? "text-red-400" : "text-neutral-200";
      if (clipBadgeRef.current)
        clipBadgeRef.current.style.display = isClipping ? "inline-flex" : "none";
    }

    rafRef.current = requestAnimationFrame(loop);
    return () => { if (rafRef.current !== null) cancelAnimationFrame(rafRef.current); };
  }, []);

  return (
    <div className="flex flex-col gap-4 rounded-xl border border-neutral-800 bg-neutral-900/70 p-5 backdrop-blur shadow-lg">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <span className="text-sm font-semibold tracking-wider uppercase text-neutral-300">
            Master Audio Meter
          </span>
          <span
            ref={clipBadgeRef}
            style={{ display: "none" }}
            className="animate-pulse rounded bg-red-600/30 px-1.5 py-0.5 text-[10px] font-bold text-red-400 border border-red-500/40"
          >
            CLIP
          </span>
        </div>
        <div className="flex items-center gap-3 font-mono text-xs text-neutral-400">
          <span>
            PEAK: <strong ref={peakLabelRef} className="text-neutral-200">-∞ dB</strong>
          </span>
          <span>
            RMS: <strong ref={rmsLabelRef} className="text-neutral-200">-∞ dB</strong>
          </span>
        </div>
      </div>

      <div className="relative flex flex-col gap-2">
        {/* Scale */}
        <div className="flex justify-between px-1 text-[10px] font-mono text-neutral-500 select-none">
          <span>-60</span><span>-48</span><span>-36</span><span>-24</span>
          <span>-18</span><span>-12</span><span>-6</span><span>-3</span>
          <span className="text-red-400 font-semibold">0</span>
        </div>

        {/* Peak bar */}
        <div className="relative h-6 w-full overflow-hidden rounded bg-neutral-950 border border-neutral-800 p-0.5">
          <div
            ref={peakBarRef}
            className="h-full rounded-sm"
            style={{
              width: "0%",
              background: "linear-gradient(90deg, #10b981 0%, #10b981 70%, #f59e0b 85%, #ef4444 100%)",
            }}
          />
          <div
            ref={holdMarkerRef}
            className="absolute top-0 bottom-0 w-0.5 bg-white shadow-sm pointer-events-none"
            style={{ display: "none", left: "0%" }}
          />
        </div>

        {/* RMS bar */}
        <div className="relative h-2 w-full overflow-hidden rounded bg-neutral-950 border border-neutral-800/80 p-[1px]">
          <div
            ref={rmsBarRef}
            className="h-full rounded-sm bg-cyan-400/80"
            style={{ width: "0%" }}
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
