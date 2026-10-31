import { useMemo, useRef } from "react";
import {
  Gamepad2,
  MessageSquare,
  Music,
  Radio,
  Volume2,
  VolumeX,
} from "lucide-react";
import { useAudioStore } from "../store/useAudioStore";
import { AudioChannel } from "../proto/routune";
import type { ChannelVolumeMetric } from "../proto/routune";

interface ChannelConfig {
  id: AudioChannel;
  name: string;
  icon: typeof Volume2;
}

// Master card first on the left
const CHANNELS: ChannelConfig[] = [
  {
    id: AudioChannel.MASTER,
    name: "Master",
    icon: Volume2,
  },
  {
    id: AudioChannel.GAME,
    name: "Game",
    icon: Gamepad2,
  },
  {
    id: AudioChannel.CHAT,
    name: "Chat",
    icon: MessageSquare,
  },
  {
    id: AudioChannel.MEDIA,
    name: "Media",
    icon: Music,
  },
  {
    id: AudioChannel.AUX,
    name: "Aux",
    icon: Radio,
  },
];

// Map -48dB .. +6dB to 0..100% position
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
  { db: 6, label: "+6" },
  { db: 0, label: "0" },
  { db: -6, label: "-6" },
  { db: -12, label: "-12" },
  { db: -24, label: "-24" },
  { db: -36, label: "-36" },
  { db: -48, label: "-48" },
];

export function MixerPanel() {
  const isConnected = useAudioStore((s) => s.connectionStatus === "connected");
  const masterPeak = useAudioStore((s) => s.peakDb);
  const masterRms = useAudioStore((s) => s.rmsDb);
  const channelMetrics = useAudioStore((s) => s.channelMetrics);
  const channelFaderDb = useAudioStore((s) => s.channelFaderDb);
  const channelMuted = useAudioStore((s) => s.channelMuted);
  const setChannelFaderDb = useAudioStore((s) => s.setChannelFaderDb);
  const toggleChannelMute = useAudioStore((s) => s.toggleChannelMute);
  const resetAllFaders = useAudioStore((s) => s.resetAllFaders);

  const metricsMap = useMemo(() => {
    const map = new Map<number, ChannelVolumeMetric>();
    for (const m of channelMetrics) {
      map.set(m.channel, m);
    }
    return map;
  }, [channelMetrics]);

  return (
    <div className="flex flex-col gap-6 w-full max-w-7xl mx-auto select-none">
      {/* Top Bar with Title and Quick Action */}
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-xl font-bold tracking-tight text-white font-sans">
            Mixer
          </h1>
        </div>

        <button
          type="button"
          onClick={resetAllFaders}
          className="rounded-xl border-2 border-white/10 bg-[#25252c] hover:bg-[#2e2e36] hover:border-white/20 hover:text-white px-3.5 py-1.5 text-xs font-mono text-zinc-200 transition-colors cursor-pointer"
        >
          Reset Flat (0.0 dB)
        </button>
      </div>

      {/* 5 Tall Vertical Channel Cards (Master first on left) */}
      <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-5 gap-4">
        {CHANNELS.map((ch) => (
          <ChannelStrip
            key={ch.id}
            config={ch}
            isConnected={isConnected}
            masterPeak={masterPeak}
            masterRms={masterRms}
            metric={metricsMap.get(ch.id)}
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

interface ChannelStripProps {
  config: ChannelConfig;
  isConnected: boolean;
  masterPeak: number;
  masterRms: number;
  metric?: ChannelVolumeMetric;
  faderDb: number;
  isMuted: boolean;
  onSetFaderDb: (db: number) => void;
  onToggleMute: () => void;
}

function ChannelStrip({
  config,
  isConnected,
  masterPeak,
  masterRms,
  metric,
  faderDb,
  isMuted,
  onSetFaderDb,
  onToggleMute,
}: ChannelStripProps) {
  const Icon = config.icon;
  const isMaster = config.id === AudioChannel.MASTER;

  const currentPeak = isMuted
    ? -100
    : isMaster
    ? masterPeak
    : metric?.peakVolumeDb ?? (isConnected ? -100 : -14.6);
  const currentRms = isMuted
    ? -100
    : isMaster
    ? masterRms
    : metric?.rmsVolumeDb ?? (isConnected ? -100 : -22.4);

  const rmsPct = isMuted ? 0 : dbToMeterPct(currentRms);
  const peakPct = isMuted ? 0 : dbToMeterPct(currentPeak);

  const faderPct = dbToFaderPct(faderDb);
  const faderText =
    faderDb > 0 ? `+${faderDb.toFixed(1)} dB` : `${faderDb.toFixed(1)} dB`;

  const isDraggingRef = useRef(false);

  const handlePointerDown = (e: React.PointerEvent<HTMLDivElement>) => {
    if (isMuted) return;
    e.preventDefault();
    e.stopPropagation();
    isDraggingRef.current = true;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);

    const rect = e.currentTarget.getBoundingClientRect();
    const clampedY = Math.max(rect.top, Math.min(rect.bottom, e.clientY));
    const pct = ((rect.bottom - clampedY) / rect.height) * 100;
    const db = faderPctToDb(pct);
    onSetFaderDb(db);
  };

  const handlePointerMove = (e: React.PointerEvent<HTMLDivElement>) => {
    if (!isDraggingRef.current || isMuted) return;
    e.preventDefault();
    const rect = e.currentTarget.getBoundingClientRect();
    const clampedY = Math.max(rect.top, Math.min(rect.bottom, e.clientY));
    const pct = ((rect.bottom - clampedY) / rect.height) * 100;
    const db = faderPctToDb(pct);
    onSetFaderDb(db);
  };

  const handlePointerUp = (e: React.PointerEvent<HTMLDivElement>) => {
    if (isDraggingRef.current) {
      isDraggingRef.current = false;
      try {
        (e.currentTarget as HTMLElement).releasePointerCapture(e.pointerId);
      } catch {}
    }
  };

  return (
    <div
      className={`flex flex-col justify-between rounded-3xl border-2 p-5 min-h-[520px] transition-colors ${
        isMaster
          ? "border-[#ff5511]/40 bg-[#23232a]"
          : "border-white/10 bg-[#202026] hover:border-white/20"
      }`}
    >
      {/* Card Header: Icon & Channel Name (No app list) */}
      <div className="flex flex-col items-center text-center gap-1.5 pb-2">
        <div className="flex h-11 w-11 items-center justify-center rounded-2xl bg-[#16161b] border-2 border-white/10 text-[#ff5511] mb-1">
          <Icon className="h-5 w-5" />
        </div>
        <h3 className="text-base font-bold text-white tracking-tight leading-tight">
          {config.name}
        </h3>
      </div>

      {/* Large Central dB Readout (No peak indicator) */}
      <div className="flex flex-col items-center justify-center my-2 py-1">
        <span
          className={`text-2xl font-bold font-mono tracking-tight leading-none ${
            isMuted ? "text-rose-400" : "text-white"
          }`}
        >
          {isMuted ? "MUTED" : faderText}
        </span>
      </div>

      {/* The Fader Track Section: Parallel Groove + Dynamic VU Meter */}
      <div className="relative flex-1 my-3 flex items-center justify-center gap-3 px-2 min-h-[290px]">
        {/* Left: Mathematical dB Markings Scale */}
        <div className="relative h-[280px] w-10 select-none font-mono text-[10px] text-zinc-400">
          {SCALE_MARKS.map((m) => {
            const pct = dbToFaderPct(m.db);
            const isZero = m.db === 0;
            return (
              <div
                key={m.db}
                className="absolute right-1 flex items-center gap-1.5 -translate-y-1/2 pointer-events-none"
                style={{ top: `${100 - pct}%` }}
              >
                <span className={isZero ? "font-bold text-zinc-200" : ""}>
                  {m.label}
                </span>
                <div
                  className={`h-[1.5px] ${
                    isZero ? "w-2.5 bg-zinc-300" : "w-1.5 bg-zinc-600"
                  }`}
                />
              </div>
            );
          })}
        </div>

        {/* Center: Dual Parallel Vertical Lines Assembly */}
        <div className="relative h-[280px] flex items-center justify-center gap-2">
          {/* Interactive Fader Touch Area (w-12 wide hitbox, exactly 280px tall) */}
          <div
            onPointerDown={handlePointerDown}
            onPointerMove={handlePointerMove}
            onPointerUp={handlePointerUp}
            onPointerCancel={handlePointerUp}
            onDoubleClick={() => !isMuted && onSetFaderDb(0.0)}
            className="relative w-12 h-[280px] flex items-center justify-center cursor-pointer touch-none select-none"
            title={`${config.name}: ${faderText} (drag to adjust, double-click for 0 dB)`}
          >
            {/* Visual Groove Line (centered inside the 48px hit area) */}
            <div className="relative w-2.5 h-[280px] rounded-full bg-[#16161b] border-2 border-white/10 pointer-events-none overflow-hidden">
              {/* Filled lower part */}
              <div
                className={`absolute bottom-0 w-full rounded-full transition-colors ${
                  isMuted ? "bg-zinc-600" : "bg-[#ff5511]"
                }`}
                style={{ height: `${faderPct}%` }}
              />
            </div>

            {/* The Fader Thumb: Single solid circle, centered mathematically on top coordinate */}
            <div
              className={`pointer-events-none absolute h-5 w-5 rounded-full border-2 transition-colors z-10 ${
                isMuted
                  ? "bg-zinc-400 border-zinc-500"
                  : "bg-white border-zinc-200"
              }`}
              style={{
                left: "50%",
                top: `${100 - faderPct}%`,
                transform: "translate(-50%, -50%)",
              }}
            />
          </div>

          {/* Dynamic LED VU Meter (No peak indicator line) */}
          <div className="relative w-2 h-[280px] rounded-full bg-[#16161b] border-2 border-white/10 overflow-hidden pointer-events-none">
            {/* RMS meter fill */}
            <div
              className="absolute bottom-0 w-full rounded-full bg-gradient-to-t from-emerald-500 via-amber-400 to-[#ff5511] opacity-75 transition-all duration-75"
              style={{ height: `${rmsPct}%` }}
            />
            {/* Peak meter fill */}
            <div
              className="absolute bottom-0 w-full rounded-full bg-gradient-to-t from-emerald-400 via-amber-300 to-[#ff5511] transition-all duration-75"
              style={{ height: `${peakPct}%` }}
            />
          </div>
        </div>
      </div>

      {/* Bottom: Wide Pill-shaped "Mute" Button */}
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
            : "border-white/10 bg-[#282830] hover:bg-[#32323c] hover:border-white/20 text-zinc-200 hover:text-white"
        }`}
      >
        {isMuted ? (
          <>
            <VolumeX className="h-4 w-4 text-rose-400" />
            <span>MUTED</span>
          </>
        ) : (
          <>
            <Volume2 className="h-4 w-4 text-zinc-400" />
            <span>Mute</span>
          </>
        )}
      </button>
    </div>
  );
}
