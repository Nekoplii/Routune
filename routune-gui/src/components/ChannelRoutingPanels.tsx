import { useMemo } from "react";
import { Gamepad2, MessageSquare, Music, Radio, Speaker, CheckCircle2, Loader2 } from "lucide-react";
import { useAudioStore } from "../store/useAudioStore";
import { AudioChannel } from "../proto/routune";
import type { ChannelVolumeMetric } from "../proto/routune";

function dbToPercent(db: number): number {
  if (db <= -60) return 0;
  if (db >= 0) return 100;
  return ((db + 60) / 60) * 100;
}

interface ChannelConfig {
  id: AudioChannel;
  name: string;
  icon: typeof Gamepad2;
  color: string;
  accentBorder: string;
  badgeBg: string;
}

const CHANNELS: ChannelConfig[] = [
  {
    id: AudioChannel.GAME,
    name: "Game",
    icon: Gamepad2,
    color: "text-emerald-400",
    accentBorder: "border-emerald-500/20",
    badgeBg: "bg-emerald-500/10 text-emerald-400 border-emerald-500/20",
  },
  {
    id: AudioChannel.CHAT,
    name: "Chat",
    icon: MessageSquare,
    color: "text-sky-400",
    accentBorder: "border-sky-500/20",
    badgeBg: "bg-sky-500/10 text-sky-400 border-sky-500/20",
  },
  {
    id: AudioChannel.MEDIA,
    name: "Media",
    icon: Music,
    color: "text-purple-400",
    accentBorder: "border-purple-500/20",
    badgeBg: "bg-purple-500/10 text-purple-400 border-purple-500/20",
  },
  {
    id: AudioChannel.AUX,
    name: "Aux",
    icon: Radio,
    color: "text-amber-400",
    accentBorder: "border-amber-500/20",
    badgeBg: "bg-amber-500/10 text-amber-400 border-amber-500/20",
  },
];

export function ChannelRoutingPanels() {
  const channelMetrics = useAudioStore((s) => s.channelMetrics);
  const hardwareOutputs = useAudioStore((s) => s.hardwareOutputs);
  const channelRoutings = useAudioStore((s) => s.channelRoutings);
  const isRoutingUpdating = useAudioStore((s) => s.isRoutingUpdating);
  const setChannelRouting = useAudioStore((s) => s.setChannelRouting);
  const isConnected = useAudioStore((s) => s.connectionStatus === "connected");

  // Lookup metric for each channel
  const metricsMap = useMemo(() => {
    const map = new Map<number, ChannelVolumeMetric>();
    for (const m of channelMetrics) {
      map.set(m.channel, m);
    }
    return map;
  }, [channelMetrics]);

  return (
    <div className="flex flex-col gap-3">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <Speaker className="h-4 w-4 text-cyan-400" />
          <h2 className="text-xs font-semibold tracking-wider uppercase text-neutral-400">
            Sonar Channel Hardware Routing (4 Virtual Sinks)
          </h2>
        </div>
        <span className="font-mono text-[11px] text-neutral-500">
          Independent CPAL Hardware Locks
        </span>
      </div>

      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-4">
        {CHANNELS.map((ch) => {
          const Icon = ch.icon;
          const metric = metricsMap.get(ch.id);
          const peakDb = isConnected && metric ? metric.peakVolumeDb : -100;
          const rmsDb = isConnected && metric ? metric.rmsVolumeDb : -100;
          const peakPct = dbToPercent(peakDb);
          const rmsPct = dbToPercent(rmsDb);
          const isClipping = peakDb >= -0.1;

          const currentDevice = channelRoutings[ch.id] || "none";
          const isUpdating = !!isRoutingUpdating[ch.id];

          return (
            <div
              key={ch.id}
              className={`flex flex-col rounded-xl border bg-neutral-900/60 p-4 shadow-sm backdrop-blur transition-all ${ch.accentBorder}`}
            >
              {/* Channel Header */}
              <div className="flex items-center justify-between pb-2 border-b border-neutral-800">
                <div className="flex items-center gap-2">
                  <Icon className={`h-4 w-4 ${ch.color}`} />
                  <span className="text-sm font-semibold text-neutral-200">
                    {ch.name}
                  </span>
                </div>
                <div className="flex items-center gap-1.5 font-mono text-[11px]">
                  <span className={isClipping ? "text-red-400 font-bold" : "text-neutral-400"}>
                    {isConnected ? `${peakDb.toFixed(1)} dB` : "-∞"}
                  </span>
                </div>
              </div>

              {/* Mini Peak/RMS Audio Bar */}
              <div className="mt-3 flex flex-col gap-1">
                <div className="relative h-3 w-full overflow-hidden rounded bg-neutral-950 border border-neutral-800/80 p-0.5">
                  <div
                    className="h-full rounded-sm transition-all duration-75 ease-out"
                    style={{
                      width: `${peakPct}%`,
                      background:
                        ch.id === AudioChannel.GAME
                          ? "linear-gradient(90deg, #10b981 0%, #10b981 70%, #f59e0b 85%, #ef4444 100%)"
                          : ch.id === AudioChannel.CHAT
                          ? "linear-gradient(90deg, #38bdf8 0%, #38bdf8 70%, #f59e0b 85%, #ef4444 100%)"
                          : ch.id === AudioChannel.MEDIA
                          ? "linear-gradient(90deg, #c084fc 0%, #c084fc 70%, #f59e0b 85%, #ef4444 100%)"
                          : "linear-gradient(90deg, #fbbf24 0%, #fbbf24 70%, #f59e0b 85%, #ef4444 100%)",
                    }}
                  />
                </div>
                <div className="relative h-1 w-full overflow-hidden rounded bg-neutral-950/60 border border-neutral-800/40">
                  <div
                    className="h-full bg-cyan-400/80 transition-all duration-100 ease-out"
                    style={{ width: `${rmsPct}%` }}
                  />
                </div>
              </div>

              {/* Hardware Device Routing Section */}
              <div className="mt-4 flex flex-col gap-1.5">
                <div className="flex items-center justify-between text-[11px] font-medium text-neutral-400">
                  <span>Target Hardware Device:</span>
                  {isUpdating ? (
                    <span className="flex items-center gap-1 text-[10px] text-indigo-400 font-mono">
                      <Loader2 className="h-2.5 w-2.5 animate-spin" /> Routing...
                    </span>
                  ) : currentDevice !== "none" && currentDevice !== "Unassigned" ? (
                    <span className="flex items-center gap-1 text-[10px] text-emerald-400 font-mono">
                      <CheckCircle2 className="h-2.5 w-2.5" /> Routed
                    </span>
                  ) : (
                    <span className="text-[10px] text-neutral-500 font-mono">
                      Muted
                    </span>
                  )}
                </div>

                <select
                  disabled={!isConnected || isUpdating}
                  value={
                    currentDevice === "none" || currentDevice === "Unassigned"
                      ? "none"
                      : hardwareOutputs.find((d) => d.id === currentDevice || d.name === currentDevice)?.id || currentDevice
                  }
                  onChange={(e) => {
                    setChannelRouting(ch.id, e.target.value);
                  }}
                  className="w-full rounded-lg border border-neutral-800 bg-neutral-950 px-2.5 py-1.5 text-xs text-neutral-200 font-mono shadow-inner transition hover:border-neutral-700 focus:border-indigo-500 focus:outline-none disabled:opacity-50 cursor-pointer"
                >
                  <option value="none">-- Unassigned (Mute) --</option>
                  {hardwareOutputs.map((device) => (
                    <option key={device.id || device.name} value={device.id || device.name}>
                      {device.displayName || device.name}
                    </option>
                  ))}
                </select>

                <div className="mt-1 flex items-center justify-between text-[10px] font-mono text-neutral-500">
                  <span>Sink: routune-{ch.name.toLowerCase()}</span>
                  <span className="truncate max-w-[140px]" title={currentDevice}>
                    {currentDevice === "none" || currentDevice === "Unassigned"
                      ? "Unbound"
                      : hardwareOutputs.find((d) => d.id === currentDevice || d.name === currentDevice)?.displayName || currentDevice}
                  </span>
                </div>
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}
