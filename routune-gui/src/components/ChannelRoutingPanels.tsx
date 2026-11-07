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
    <div className="flex flex-col gap-4">
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2">
          <Speaker className="h-4 w-4 text-sky-400" />
          <h2 className="text-xs font-bold tracking-wider uppercase text-zinc-300 font-mono">
            Hardware Output Sinks (4 Virtual Sinks)
          </h2>
        </div>
        <span className="font-mono text-[11px] text-zinc-400">
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
              className={`flex flex-col rounded-2xl border bg-[#171528] p-4 shadow-xl backdrop-blur-md transition-all ${ch.accentBorder} hover:border-[#3F3B5F]`}
            >
              {/* Channel Header */}
              <div className="flex items-center justify-between pb-3 border-b border-[#2B2942]/60">
                <div className="flex items-center gap-2">
                  <div className="flex h-8 w-8 items-center justify-center rounded-lg bg-[#0F0E17] border border-[#2B2942]/60">
                    <Icon className={`h-4 w-4 ${ch.color}`} />
                  </div>
                  <div>
                    <span className="text-sm font-bold text-[#ECEAFB] block leading-none">
                      {ch.name}
                    </span>
                    <span className="text-[10px] font-mono text-[#ECEAFB]/50">Virtual Sink</span>
                  </div>
                </div>
                <div className="flex items-center gap-1.5 font-mono text-xs">
                  <span className={isClipping ? "text-rose-400 font-bold" : "text-[#ECEAFB]/60"}>
                    {isConnected ? `${peakDb.toFixed(1)} dB` : "-∞ dB"}
                  </span>
                </div>
              </div>

              {/* Mini Peak/RMS Audio Bar */}
              <div className="mt-3 flex flex-col gap-1.5">
                <div className="relative h-2.5 w-full overflow-hidden rounded bg-[#0F0E17] border border-[#2B2942]/60 p-0.5">
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
                <div className="relative h-1 w-full overflow-hidden rounded bg-[#0F0E17] border border-[#2B2942]/60">
                  <div
                    className="h-full bg-[#7C6AF2]/80 transition-all duration-100 ease-out"
                    style={{ width: `${rmsPct}%` }}
                  />
                </div>
              </div>

              {/* Hardware Device Routing Section */}
              <div className="mt-4 flex flex-col gap-1.5">
                <div className="flex items-center justify-between text-[11px] font-mono font-medium text-[#ECEAFB]/60">
                  <span>TARGET HARDWARE</span>
                  {isUpdating ? (
                    <span className="flex items-center gap-1 text-[10px] text-[#7C6AF2] font-mono">
                      <Loader2 className="h-2.5 w-2.5 animate-spin" /> Routing...
                    </span>
                  ) : currentDevice !== "none" && currentDevice !== "Unassigned" ? (
                    <span className="flex items-center gap-1 text-[10px] text-teal-400 font-mono">
                      <CheckCircle2 className="h-2.5 w-2.5" /> Bound
                    </span>
                  ) : (
                    <span className="text-[10px] text-[#ECEAFB]/40 font-mono">
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
                  className="w-full rounded-xl border border-[#2B2942] bg-[#0F0E17] px-3 py-2 text-xs text-[#ECEAFB] font-mono shadow-inner transition hover:border-[#3F3B5F] focus:border-[#7C6AF2] focus:outline-none disabled:opacity-50 cursor-pointer"
                >
                  <option value="none">-- Unassigned (Mute) --</option>
                  {hardwareOutputs.map((device) => (
                    <option key={device.id || device.name} value={device.id || device.name}>
                      {device.displayName || device.name}
                    </option>
                  ))}
                </select>

                <div className="mt-1 flex items-center justify-between text-[10px] font-mono text-[#ECEAFB]/50">
                  <span>routune-{ch.name.toLowerCase()}</span>
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
