import { useRef, useCallback } from "react";
import {
  Activity,
  RotateCcw,
  Gamepad2,
  MessageSquare,
  Music,
  Radio,
  Sparkles,
} from "lucide-react";
import { useAudioStore } from "../store/useAudioStore";
import { AudioChannel } from "../proto/routune";
import { routuneClient } from "../services/grpcClient";

export interface EqBandDef {
  index: number;
  freqHz: number;
  label: string;
}

export const EQ_BANDS: EqBandDef[] = [
  { index: 0, freqHz: 31, label: "31Hz" },
  { index: 1, freqHz: 62, label: "62Hz" },
  { index: 2, freqHz: 125, label: "125Hz" },
  { index: 3, freqHz: 250, label: "250Hz" },
  { index: 4, freqHz: 500, label: "500Hz" },
  { index: 5, freqHz: 1000, label: "1kHz" },
  { index: 6, freqHz: 2000, label: "2kHz" },
  { index: 7, freqHz: 4000, label: "4kHz" },
  { index: 8, freqHz: 8000, label: "8kHz" },
  { index: 9, freqHz: 16000, label: "16kHz" },
];

const CHANNEL_TABS = [
  {
    id: AudioChannel.GAME,
    name: "Game",
    icon: Gamepad2,
    activeColor: "text-emerald-400",
    activeBorder: "border-emerald-500/40",
    activeBg: "bg-emerald-500/10",
    thumbClass: "text-emerald-400",
  },
  {
    id: AudioChannel.CHAT,
    name: "Chat",
    icon: MessageSquare,
    activeColor: "text-sky-400",
    activeBorder: "border-sky-500/40",
    activeBg: "bg-sky-500/10",
    thumbClass: "text-sky-400",
  },
  {
    id: AudioChannel.MEDIA,
    name: "Media",
    icon: Music,
    activeColor: "text-purple-400",
    activeBorder: "border-purple-500/40",
    activeBg: "bg-purple-500/10",
    thumbClass: "text-purple-400",
  },
  {
    id: AudioChannel.AUX,
    name: "Aux",
    icon: Radio,
    activeColor: "text-amber-400",
    activeBorder: "border-amber-500/40",
    activeBg: "bg-amber-500/10",
    thumbClass: "text-amber-400",
  },
];

export function EqualizerPanel() {
  const selectedChannel = useAudioStore((s) => s.selectedEqChannel);
  const setSelectedChannel = useAudioStore((s) => s.setSelectedEqChannel);
  const eqBands = useAudioStore((s) => s.eqBands);
  const setEqBandGain = useAudioStore((s) => s.setEqBandGain);
  const resetChannelEq = useAudioStore((s) => s.resetChannelEq);
  const isConnected = useAudioStore((s) => s.connectionStatus === "connected");
  const channelNoiseEnabled = useAudioStore((s) => s.channelNoiseEnabled);
  const toggleAiNoise = useAudioStore((s) => s.toggleAiNoise);

  const activeChannelConfig =
    CHANNEL_TABS.find((t) => t.id === selectedChannel) || CHANNEL_TABS[0];
  const channelGains = eqBands[selectedChannel] || [0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
  const isAiNoiseActive = channelNoiseEnabled[selectedChannel] ?? false;

  // Debounce timers keyed by `${channel}-${bandIndex}`
  const debounceTimers = useRef<Record<string, ReturnType<typeof setTimeout>>>({});

  const dispatchSetEqBand = useCallback(
    (channel: AudioChannel, bandIndex: number, gainDb: number) => {
      // 1. Immediately update UI state (lock-free feel, 60fps)
      setEqBandGain(channel, bandIndex, gainDb);

      // 2. Debounce IPC gRPC call by 50ms to prevent stream saturation during drag
      const key = `${channel}-${bandIndex}`;
      if (debounceTimers.current[key]) {
        clearTimeout(debounceTimers.current[key]);
      }

      debounceTimers.current[key] = setTimeout(() => {
        routuneClient
          .setEqBand({
            channel,
            bandIndex,
            gainDb,
          })
          .response.catch((err) => {
            console.warn(
              `Failed to send SetEqBand for ch=${channel} band=${bandIndex}:`,
              err
            );
          });
        delete debounceTimers.current[key];
      }, 50);
    },
    [setEqBandGain]
  );

  const handleResetFlat = useCallback(() => {
    resetChannelEq(selectedChannel);
    // Send 0.0 dB for all 10 bands immediately
    for (let i = 0; i < 10; i++) {
      const key = `${selectedChannel}-${i}`;
      if (debounceTimers.current[key]) {
        clearTimeout(debounceTimers.current[key]);
        delete debounceTimers.current[key];
      }
      routuneClient
        .setEqBand({
          channel: selectedChannel,
          bandIndex: i,
          gainDb: 0.0,
        })
        .response.catch((err) => {
          console.warn(`Failed to reset EQ band ${i}:`, err);
        });
    }
  }, [resetChannelEq, selectedChannel]);

  return (
    <div className="flex flex-col rounded-2xl border border-white/5 bg-zinc-900/60 p-5 shadow-xl backdrop-blur-md">
      {/* Header & Channel Selector */}
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4 border-b border-white/5 pb-4">
        <div className="flex items-center gap-3">
          <div className="flex h-10 w-10 items-center justify-center rounded-xl border border-white/5 bg-zinc-950 text-purple-400 shadow-inner">
            <Activity className="h-5 w-5" />
          </div>
          <div>
            <div className="flex items-center gap-2">
              <h2 className="text-sm font-bold tracking-tight text-white">
                Acoustic Equalizer & Noise Cancellation
              </h2>
              <span className="rounded bg-white/5 border border-white/10 px-1.5 py-0.5 font-mono text-[10px] text-zinc-300">
                10-Band Biquad
              </span>
            </div>
            <p className="text-xs text-zinc-400 mt-0.5">
              DirectForm1 cascaded peaking filters (Q ≈ 1.414, 48kHz lock-free)
            </p>
          </div>
        </div>

        {/* Right Actions: AI Noise Suppression Toggle & Reset Flat */}
        <div className="flex items-center gap-2.5 self-start sm:self-auto">
          {/* AI Noise Suppression (RNNoise) Switch */}
          <button
            onClick={() => toggleAiNoise(selectedChannel, !isAiNoiseActive)}
            disabled={!isConnected}
            title={
              isAiNoiseActive
                ? `Disable AI Noise Suppression for ${activeChannelConfig.name}`
                : `Enable AI Noise Suppression (RNNoise) for ${activeChannelConfig.name}`
            }
            className={`flex items-center gap-2 rounded-xl border px-3 py-2 text-xs font-medium transition-all cursor-pointer disabled:opacity-40 disabled:cursor-not-allowed select-none ${
              isAiNoiseActive
                ? "border-emerald-500/50 bg-emerald-500/15 text-emerald-300 shadow-[0_0_15px_rgba(16,185,129,0.2)]"
                : "border-white/10 bg-zinc-800/60 text-zinc-400 hover:border-white/20 hover:text-zinc-200"
            }`}
          >
            <Sparkles
              className={`h-3.5 w-3.5 transition-transform duration-200 ${
                isAiNoiseActive ? "text-emerald-400 scale-110" : "text-zinc-500"
              }`}
            />
            <span className="hidden sm:inline font-medium">AI Noise Suppression (RNNoise)</span>
            <span className="inline sm:hidden font-medium">AI Noise</span>
            <div
              className={`relative inline-flex h-4 w-7 shrink-0 items-center rounded-full transition-colors duration-200 ease-in-out ${
                isAiNoiseActive ? "bg-emerald-500" : "bg-zinc-700"
              }`}
            >
              <span
                className={`inline-block h-2.5 w-2.5 transform rounded-full bg-white shadow transition-transform duration-200 ease-in-out ${
                  isAiNoiseActive ? "translate-x-3.5" : "translate-x-0.5"
                }`}
              />
            </div>
          </button>

          {/* Reset Flat */}
          <button
            onClick={handleResetFlat}
            disabled={!isConnected}
            title="Reset all bands to 0 dB"
            className="flex items-center gap-1.5 rounded-xl border border-white/10 bg-zinc-800/80 px-3 py-2 text-xs font-mono font-medium text-zinc-300 transition-colors hover:border-white/20 hover:bg-zinc-700/80 hover:text-white disabled:opacity-40 disabled:cursor-not-allowed cursor-pointer"
          >
            <RotateCcw className="h-3.5 w-3.5" />
            <span>Reset Flat</span>
          </button>
        </div>
      </div>

      {/* Channel Selection Tabs */}
      <div className="flex items-center gap-2 pt-4 pb-2 overflow-x-auto">
        <span className="text-xs font-medium text-zinc-400 mr-1 shrink-0 font-mono">
          CHANNEL:
        </span>
        <div className="flex items-center gap-1.5 rounded-xl border border-white/5 bg-zinc-950/80 p-1 shrink-0">
          {CHANNEL_TABS.map((tab) => {
            const Icon = tab.icon;
            const isSelected = tab.id === selectedChannel;
            return (
              <button
                key={tab.id}
                onClick={() => setSelectedChannel(tab.id)}
                className={`flex items-center gap-2 rounded-lg px-3 py-1.5 text-xs font-medium transition-all cursor-pointer ${
                  isSelected
                    ? `${tab.activeBg} ${tab.activeColor} ${tab.activeBorder} border shadow-sm`
                    : "text-zinc-400 border border-transparent hover:text-zinc-200 hover:bg-zinc-900/80"
                }`}
              >
                <Icon className="h-3.5 w-3.5" />
                <span>{tab.name}</span>
              </button>
            );
          })}
        </div>
      </div>

      {/* EQ Sliders Grid */}
      <div className="relative mt-4 rounded-xl border border-white/5 bg-zinc-950/60 p-4">
        {/* Background zero dB reference line */}
        <div
          className="pointer-events-none absolute left-10 right-10 border-b border-dashed border-white/10"
          style={{ top: "calc(50% + 2px)" }}
        />

        <div className="grid grid-cols-10 gap-1 sm:gap-2 text-center">
          {EQ_BANDS.map((band) => {
            const gain = channelGains[band.index] ?? 0.0;
            const isZero = Math.abs(gain) < 0.05;
            const isPositive = gain > 0.05;

            return (
              <div
                key={band.index}
                className="flex flex-col items-center group select-none"
              >
                {/* Gain Readout Badge */}
                <button
                  type="button"
                  onClick={() => dispatchSetEqBand(selectedChannel, band.index, 0.0)}
                  title="Click to reset to 0.0 dB"
                  className={`h-5 w-full flex items-center justify-center font-mono text-[11px] rounded transition-colors cursor-pointer ${
                    isZero
                      ? "text-neutral-400 group-hover:text-neutral-300"
                      : isPositive
                      ? `${activeChannelConfig.activeColor} font-semibold`
                      : "text-neutral-300 font-semibold"
                  }`}
                >
                  {isPositive ? `+${gain.toFixed(1)}` : gain.toFixed(1)}
                </button>

                {/* Vertical Range Slider */}
                <div
                  className="relative my-2 flex items-center justify-center h-[140px]"
                  onDoubleClick={() =>
                    dispatchSetEqBand(selectedChannel, band.index, 0.0)
                  }
                >
                  <input
                    type="range"
                    min="-12.0"
                    max="12.0"
                    step="0.1"
                    value={gain}
                    disabled={!isConnected}
                    onChange={(e) => {
                      const val = parseFloat(e.target.value);
                      dispatchSetEqBand(selectedChannel, band.index, val);
                    }}
                    className={`eq-slider-vertical ${activeChannelConfig.thumbClass} disabled:opacity-40 disabled:cursor-not-allowed`}
                    title={`${band.label}: ${gain.toFixed(1)} dB (double click to reset)`}
                  />
                </div>

                {/* Frequency Band Label */}
                <span className="font-mono text-[10px] tracking-tight text-neutral-400 group-hover:text-neutral-200 transition-colors">
                  {band.label}
                </span>
              </div>
            );
          })}
        </div>

        {/* Range Bounds Indicators */}
        <div className="mt-3 pt-3 border-t border-neutral-800/60 flex items-center justify-between text-[11px] font-mono text-neutral-400">
          <div className="flex items-center gap-3">
            <span className="text-neutral-400">Range:</span>
            <span>-12.0 dB ~ +12.0 dB</span>
          </div>
          <div className="flex items-center gap-2 text-neutral-400">
            <span>Center:</span>
            <span className="text-neutral-400 font-medium">0.0 dB Flat</span>
          </div>
          <div className="text-neutral-400">
            Double-click slider to reset band
          </div>
        </div>
      </div>
    </div>
  );
}
