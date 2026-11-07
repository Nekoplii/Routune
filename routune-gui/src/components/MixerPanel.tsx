import { useRef, useEffect, useCallback, memo } from "react";
import {
  Gamepad2,
  MessageSquare,
  Mic,
  Music,
  Radio,
  Volume2,
  VolumeX,
} from "lucide-react";
import { useAudioStore } from "../store/useAudioStore";
import { AudioChannel } from "../proto/routune";

interface ChannelConfig {
  id: AudioChannel;
  name: string;
  icon: typeof Volume2;
}

const CHANNELS: ChannelConfig[] = [
  { id: AudioChannel.MIC,    name: "Mic",    icon: Mic },
  { id: AudioChannel.MASTER, name: "Master", icon: Volume2 },
  { id: AudioChannel.GAME,   name: "Game",   icon: Gamepad2 },
  { id: AudioChannel.CHAT,   name: "Chat",   icon: MessageSquare },
  { id: AudioChannel.MEDIA,  name: "Media",  icon: Music },
  { id: AudioChannel.AUX,    name: "Aux",    icon: Radio },
];

function dbToFaderPct(db: number): number {
  if (db <= -48) return 0;
  if (db >= 6) return 100;
  return ((db + 48) / 54) * 100;
}

function faderPctToDb(pct: number): number {
  const clamped = Math.max(0, Math.min(100, pct));
  const db = (clamped / 100) * 54 - 48;
  return Math.round(db * 10) / 10;
}

function dbToMeterPct(db: number): number {
  if (db <= -60) return 0;
  if (db >= 0) return 100;
  return ((db + 60) / 60) * 100;
}

const SCALE_MARKS = [
  { db: 6,   label: "+6"  },
  { db: 0,   label: "0"   },
  { db: -6,  label: "-6"  },
  { db: -12, label: "-12" },
  { db: -24, label: "-24" },
  { db: -36, label: "-36" },
  { db: -48, label: "-48" },
];

// Canvas VU meter — subscribes to store directly, never causes React re-renders
function ChannelVuMeter({ channelId, isMuted }: { channelId: AudioChannel; isMuted: boolean }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const isMutedRef = useRef(isMuted);
  const rafRef = useRef<number | null>(null);
  const peakHoldRef = useRef(-100);
  const peakHoldTimerRef = useRef(0);

  useEffect(() => {
    isMutedRef.current = isMuted;
  }, [isMuted]);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;

    const isMaster = channelId === AudioChannel.MASTER;

    // Read latest meter values from store without subscribing (no re-render)
    function getValues(): { peak: number; rms: number } {
      if (isMutedRef.current) return { peak: -100, rms: -100 };
      const state = useAudioStore.getState();
      if (state.connectionStatus !== "connected") return { peak: -100, rms: -100 };
      if (isMaster) {
        return { peak: state.peakDb, rms: state.rmsDb };
      }
      const metric = state.channelMetrics.find((m) => m.channel === channelId);
      return {
        peak: metric?.peakVolumeDb ?? -100,
        rms:  metric?.rmsVolumeDb  ?? -100,
      };
    }

    function draw() {
      rafRef.current = requestAnimationFrame(draw);
      if (!canvas || !ctx) return;

      const { peak, rms } = getValues();
      const W = canvas.width;
      const H = canvas.height;

      // Peak hold
      const now = performance.now();
      if (peak >= peakHoldRef.current) {
        peakHoldRef.current = peak;
        peakHoldTimerRef.current = now;
      } else if (now - peakHoldTimerRef.current > 1200) {
        peakHoldRef.current = Math.max(-100, peakHoldRef.current - 0.5);
      }

      const rmsPct  = dbToMeterPct(rms)  / 100;
      const peakPct = dbToMeterPct(peak) / 100;
      const holdPct = dbToMeterPct(peakHoldRef.current) / 100;

      ctx.clearRect(0, 0, W, H);

      // Background
      ctx.fillStyle = "#0F0E17";
      ctx.beginPath();
      ctx.roundRect(0, 0, W, H, W / 2);
      ctx.fill();

      // RMS fill (bottom-up, 75% opacity gradient)
      const rmsGrad = ctx.createLinearGradient(0, H, 0, 0);
      rmsGrad.addColorStop(0,    "rgba(16,185,129,0.75)");  // emerald-500
      rmsGrad.addColorStop(0.65, "rgba(251,191,36,0.75)");  // amber-400
      rmsGrad.addColorStop(1,    "rgba(124,106,242,0.85)"); // primary violet
      ctx.fillStyle = rmsGrad;
      ctx.beginPath();
      ctx.roundRect(0, H * (1 - rmsPct), W, H * rmsPct, W / 2);
      ctx.fill();

      // Peak fill (on top, full opacity)
      const pkGrad = ctx.createLinearGradient(0, H, 0, 0);
      pkGrad.addColorStop(0,    "#10b981");  // emerald-400
      pkGrad.addColorStop(0.65, "#fbbf24");  // amber-300
      pkGrad.addColorStop(1,    "#7C6AF2");
      ctx.fillStyle = pkGrad;
      ctx.beginPath();
      ctx.roundRect(0, H * (1 - peakPct), W, H * peakPct, W / 2);
      ctx.fill();

      // Peak hold line
      if (holdPct > 0) {
        const y = H * (1 - holdPct);
        ctx.strokeStyle = "rgba(236,234,251,0.8)";
        ctx.lineWidth = 1.5;
        ctx.beginPath();
        ctx.moveTo(0, y);
        ctx.lineTo(W, y);
        ctx.stroke();
      }
    }

    rafRef.current = requestAnimationFrame(draw);
    return () => {
      if (rafRef.current !== null) cancelAnimationFrame(rafRef.current);
    };
  }, [channelId]);

  return (
    <canvas
      ref={canvasRef}
      width={8}
      height={280}
      className="rounded-full border-2 border-[#2B2942] pointer-events-none"
      style={{ width: 8, height: 280 }}
    />
  );
}

// Isolated strip — only re-renders on fader/mute changes, never on telemetry
interface ChannelStripProps {
  config: ChannelConfig;
  faderDb: number;
  isMuted: boolean;
  onSetFaderDb: (db: number) => void;
  onToggleMute: () => void;
}

const ChannelStrip = memo(function ChannelStrip({
  config,
  faderDb,
  isMuted,
  onSetFaderDb,
  onToggleMute,
}: ChannelStripProps) {
  const Icon = config.icon;
  const faderPct = dbToFaderPct(faderDb);
  const faderText = faderDb > 0 ? `+${faderDb.toFixed(1)} dB` : `${faderDb.toFixed(1)} dB`;
  const isDraggingRef = useRef(false);

  const handlePointerDown = useCallback((e: React.PointerEvent<HTMLDivElement>) => {
    if (isMuted) return;
    e.preventDefault();
    e.stopPropagation();
    isDraggingRef.current = true;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    const rect = e.currentTarget.getBoundingClientRect();
    const pct = ((rect.bottom - Math.max(rect.top, Math.min(rect.bottom, e.clientY))) / rect.height) * 100;
    onSetFaderDb(faderPctToDb(pct));
  }, [isMuted, onSetFaderDb]);

  const handlePointerMove = useCallback((e: React.PointerEvent<HTMLDivElement>) => {
    if (!isDraggingRef.current || isMuted) return;
    e.preventDefault();
    const rect = e.currentTarget.getBoundingClientRect();
    const pct = ((rect.bottom - Math.max(rect.top, Math.min(rect.bottom, e.clientY))) / rect.height) * 100;
    onSetFaderDb(faderPctToDb(pct));
  }, [isMuted, onSetFaderDb]);

  const handlePointerUp = useCallback((e: React.PointerEvent<HTMLDivElement>) => {
    if (isDraggingRef.current) {
      isDraggingRef.current = false;
      try { (e.currentTarget as HTMLElement).releasePointerCapture(e.pointerId); } catch {}
    }
  }, []);

  const isMaster = config.id === AudioChannel.MASTER;

  return (
    <div
      className={`flex flex-col justify-between rounded-3xl border-2 p-5 min-h-[520px] transition-colors ${
        isMaster
          ? "border-[#7C6AF2]/60 bg-[#1A172E] shadow-xl shadow-[#7C6AF2]/10"
          : "border-[#2B2942] bg-[#171528] hover:border-[#3F3B5F]"
      }`}
    >
      {/* Header */}
      <div className="flex flex-col items-center text-center gap-1.5 pb-2">
        <div className="flex h-11 w-11 items-center justify-center rounded-2xl bg-[#0F0E17] border-2 border-[#2B2942] text-[#7C6AF2] mb-1">
          <Icon className="h-5 w-5" />
        </div>
        <h3 className="text-base font-bold text-[#ECEAFB] tracking-tight leading-tight">
          {config.name}
        </h3>
      </div>

      {/* dB readout */}
      <div className="flex flex-col items-center justify-center my-2 py-1">
        <span
          className={`text-2xl font-bold font-mono tracking-tight leading-none ${
            isMuted ? "text-rose-400" : "text-[#ECEAFB]"
          }`}
        >
          {isMuted ? "MUTED" : faderText}
        </span>
      </div>

      {/* Fader + VU meter */}
      <div className="relative flex-1 my-3 flex items-center justify-center gap-3 px-2 min-h-[290px]">
        {/* Scale */}
        <div className="relative h-[280px] w-10 select-none font-mono text-[10px] text-[#ECEAFB]/50">
          {SCALE_MARKS.map((m) => {
            const pct = dbToFaderPct(m.db);
            return (
              <div
                key={m.db}
                className="absolute right-1 flex items-center gap-1.5 -translate-y-1/2 pointer-events-none"
                style={{ top: `${100 - pct}%` }}
              >
                <span className={m.db === 0 ? "font-bold text-[#ECEAFB]" : ""}>{m.label}</span>
                <div className={`h-[1.5px] ${m.db === 0 ? "w-2.5 bg-[#C4B5FD]" : "w-1.5 bg-[#2B2942]"}`} />
              </div>
            );
          })}
        </div>

        {/* Fader + canvas VU side by side */}
        <div className="relative h-[280px] flex items-center justify-center gap-2">
          {/* Fader hit area */}
          <div
            onPointerDown={handlePointerDown}
            onPointerMove={handlePointerMove}
            onPointerUp={handlePointerUp}
            onPointerCancel={handlePointerUp}
            onDoubleClick={() => !isMuted && onSetFaderDb(0.0)}
            className="relative w-12 h-[280px] flex items-center justify-center cursor-pointer touch-none select-none"
            title={`${config.name}: ${faderText} (drag to adjust, double-click for 0 dB)`}
          >
            {/* Groove */}
            <div className="relative w-2.5 h-[280px] rounded-full bg-[#0F0E17] border-2 border-[#2B2942] pointer-events-none overflow-hidden">
              <div
                className={`absolute bottom-0 w-full rounded-full transition-colors ${
                  isMuted ? "bg-zinc-600" : "bg-[#7C6AF2]"
                }`}
                style={{ height: `${faderPct}%` }}
              />
            </div>
            {/* Thumb */}
            <div
              className={`pointer-events-none absolute h-5 w-5 rounded-full border-2 transition-colors z-10 ${
                isMuted ? "bg-zinc-500 border-zinc-600" : "bg-[#C4B5FD] border-[#ECEAFB] shadow-md shadow-[#7C6AF2]/30"
              }`}
              style={{
                left: "50%",
                top: `${100 - faderPct}%`,
                transform: "translate(-50%, -50%)",
              }}
            />
          </div>

          {/* Canvas VU meter — zero React re-renders at 60fps */}
          <ChannelVuMeter channelId={config.id} isMuted={isMuted} />
        </div>
      </div>

      {/* Mute button */}
      <button
        type="button"
        onPointerDown={(e) => e.stopPropagation()}
        onClick={(e) => {
          e.preventDefault();
          e.stopPropagation();
          onToggleMute();
        }}
        className={`w-full flex items-center justify-center gap-2 rounded-full py-3 px-4 text-xs font-semibold tracking-wider transition-colors cursor-pointer border-2 ${
          isMuted
            ? "bg-rose-500/20 border-rose-500 text-rose-300 animate-pulse"
            : "border-[#2B2942] bg-[#201D36] hover:bg-[#282444] hover:border-[#3F3B5F] text-[#ECEAFB]"
        }`}
      >
        {isMuted ? (
          <>
            <VolumeX className="h-4 w-4 text-rose-400" />
            <span>MUTED</span>
          </>
        ) : (
          <>
            <Volume2 className="h-4 w-4 text-[#ECEAFB]/60" />
            <span>Mute</span>
          </>
        )}
      </button>
    </div>
  );
});

export function MixerPanel() {
  // These selectors only change on user interaction, not on telemetry ticks
  const channelFaderDb = useAudioStore((s) => s.channelFaderDb);
  const channelMuted   = useAudioStore((s) => s.channelMuted);
  const setChannelFaderDb = useAudioStore((s) => s.setChannelFaderDb);
  const toggleChannelMute = useAudioStore((s) => s.toggleChannelMute);
  const resetAllFaders    = useAudioStore((s) => s.resetAllFaders);

  return (
    <div className="flex flex-col gap-6 w-full max-w-7xl mx-auto select-none">
      <div className="flex items-center justify-between">
        <h1 className="text-xl font-bold tracking-tight text-[#ECEAFB] font-sans">Mixer</h1>
        <button
          type="button"
          onClick={resetAllFaders}
          className="rounded-xl border-2 border-[#2B2942] bg-[#171528] hover:bg-[#201D36] hover:border-[#3F3B5F] hover:text-white px-3.5 py-1.5 text-xs font-mono text-[#ECEAFB]/80 transition-colors cursor-pointer"
        >
          Reset Flat (0.0 dB)
        </button>
      </div>

      <div className="grid grid-cols-1 sm:grid-cols-2 md:grid-cols-3 xl:grid-cols-6 gap-4">
        {CHANNELS.map((ch) => (
          <ChannelStrip
            key={ch.id}
            config={ch}
            faderDb={channelFaderDb[ch.id] ?? 0.0}
            isMuted={Boolean(channelMuted[ch.id])}
            onSetFaderDb={(db) => setChannelFaderDb(ch.id, db)}
            onToggleMute={() => toggleChannelMute(ch.id)}
          />
        ))}
      </div>
    </div>
  );
}
