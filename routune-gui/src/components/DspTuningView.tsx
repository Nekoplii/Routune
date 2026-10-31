import { useState, useRef, useCallback, useMemo } from "react";
import {
  Gamepad2,
  MessageSquare,
  Music,
  Radio,
  RotateCcw,
  AudioWaveform,
  Sliders,
} from "lucide-react";
import { useAudioStore } from "../store/useAudioStore";
import { AudioChannel } from "../proto/routune";
import { routuneClient } from "../services/grpcClient";

interface ChannelTab {
  id: AudioChannel;
  name: string;
  icon: typeof Gamepad2;
}

const CHANNELS: ChannelTab[] = [
  { id: AudioChannel.GAME, name: "Game", icon: Gamepad2 },
  { id: AudioChannel.CHAT, name: "Chat", icon: MessageSquare },
  { id: AudioChannel.MEDIA, name: "Media", icon: Music },
  { id: AudioChannel.AUX, name: "Aux", icon: Radio },
];

const FREQ_LABELS = ["31", "62", "125", "250", "500", "1k", "2k", "4k", "8k", "16k"];

interface Point {
  x: number;
  y: number;
}

interface EqPreset {
  name: string;
  gains: number[];
}

const EQ_PRESETS: EqPreset[] = [
  { name: "Flat", gains: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0] },
  { name: "Bass Boost", gains: [5.5, 4.0, 2.5, 1.0, 0, 0, 0, 0, 0, 0] },
  { name: "FPS Footsteps", gains: [-2.0, -1.0, 0, 0, 1.5, 3.5, 5.5, 6.0, 3.5, 1.0] },
  { name: "Vocal Clarity", gains: [-3.5, -1.5, 0, 1.0, 2.5, 3.5, 2.5, 1.0, 0, -1.0] },
  { name: "Smiley / V-Curve", gains: [4.5, 3.0, 1.0, -1.0, -2.0, -2.0, 0, 2.0, 4.0, 5.0] },
  { name: "Cinema / Movie", gains: [5.0, 3.5, 1.5, 0, -1.0, 1.0, 2.0, 3.0, 3.5, 2.5] },
];

const BAND_SPACING = 92.2;
const Y_BASELINE = 160;

/**
 * Fritsch-Carlson Monotone Cubic Spline interpolation.
 * Guarantees the curve passes strictly through all control points (nodes)
 * while preserving local monotonicity with zero artificial overshoot, clipping,
 * or table-top plateaus.
 */
function getMonotoneSplinePaths(points: Point[]): { curvePath: string; fillPath: string } {
  if (points.length === 0) return { curvePath: "", fillPath: "" };
  if (points.length === 1) return { curvePath: `M ${points[0].x} ${points[0].y}`, fillPath: "" };

  const n = points.length;
  const dxs: number[] = [];
  const dys: number[] = [];
  const slopes: number[] = [];

  for (let i = 0; i < n - 1; i++) {
    const dx = points[i + 1].x - points[i].x;
    const dy = points[i + 1].y - points[i].y;
    dxs.push(dx);
    dys.push(dy);
    slopes.push(dx === 0 ? 0 : dy / dx);
  }

  const m: number[] = new Array(n).fill(0);
  m[0] = slopes[0] || 0;
  m[n - 1] = slopes[n - 2] || 0;

  for (let i = 1; i < n - 1; i++) {
    const s0 = slopes[i - 1];
    const s1 = slopes[i];
    // Local extremum: tangent is strictly zero so peak/valley is centered on node
    if (s0 * s1 <= 0) {
      m[i] = 0;
    } else {
      m[i] = (s0 + s1) / 2;
    }
  }

  // Fritsch-Carlson adjustment to prevent overshoot
  for (let i = 0; i < n - 1; i++) {
    const s = slopes[i];
    if (Math.abs(s) < 1e-6) {
      m[i] = 0;
      m[i + 1] = 0;
      continue;
    }
    const alpha = m[i] / s;
    const beta = m[i + 1] / s;
    const distSq = alpha * alpha + beta * beta;
    if (distSq > 9) {
      const tau = 3 / Math.sqrt(distSq);
      m[i] = tau * alpha * s;
      m[i + 1] = tau * beta * s;
    }
  }

  let curvePath = `M ${points[0].x.toFixed(1)} ${points[0].y.toFixed(1)}`;
  for (let i = 0; i < n - 1; i++) {
    const p1 = points[i];
    const p2 = points[i + 1];
    const dx = dxs[i];

    const cp1x = p1.x + dx / 3;
    const cp1y = p1.y + (m[i] * dx) / 3;

    const cp2x = p2.x - dx / 3;
    const cp2y = p2.y - (m[i + 1] * dx) / 3;

    curvePath += ` C ${cp1x.toFixed(1)} ${cp1y.toFixed(1)}, ${cp2x.toFixed(1)} ${cp2y.toFixed(1)}, ${p2.x.toFixed(1)} ${p2.y.toFixed(1)}`;
  }

  const firstX = points[0].x.toFixed(1);
  const lastX = points[n - 1].x.toFixed(1);
  const fillPath = `${curvePath} L ${lastX} ${Y_BASELINE} L ${firstX} ${Y_BASELINE} Z`;

  return { curvePath, fillPath };
}

export function DspTuningView() {
  const selectedChannel = useAudioStore((s) => s.selectedEqChannel);
  const setSelectedChannel = useAudioStore((s) => s.setSelectedEqChannel);
  const eqBands = useAudioStore((s) => s.eqBands);
  const setEqBandGain = useAudioStore((s) => s.setEqBandGain);
  const resetChannelEq = useAudioStore((s) => s.resetChannelEq);
  const isConnected = useAudioStore((s) => s.connectionStatus === "connected");
  const channelNoiseEnabled = useAudioStore((s) => s.channelNoiseEnabled);
  const toggleAiNoise = useAudioStore((s) => s.toggleAiNoise);

  const activeChannel = CHANNELS.find((c) => c.id === selectedChannel) || CHANNELS[0];
  const channelGains = eqBands[selectedChannel] || [0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
  const isAiNoiseActive = channelNoiseEnabled[selectedChannel] ?? false;

  const svgRef = useRef<SVGSVGElement | null>(null);
  const [draggingIndex, setDraggingIndex] = useState<number | null>(null);
  const [hoveredBand, setHoveredBand] = useState<number | null>(null);

  // Debounce timers for IPC calls
  const debounceTimers = useRef<Record<string, ReturnType<typeof setTimeout>>>({});

  const dispatchSetEqBand = useCallback(
    (channel: AudioChannel, bandIndex: number, gainDb: number) => {
      setEqBandGain(channel, bandIndex, gainDb);

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
            console.warn(`Failed to send SetEqBand for ch=${channel} band=${bandIndex}:`, err);
          });
        delete debounceTimers.current[key];
      }, 50);
    },
    [setEqBandGain]
  );

  const handleResetFlat = useCallback(() => {
    resetChannelEq(selectedChannel);
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
        .response.catch(() => {});
    }
  }, [resetChannelEq, selectedChannel]);

  const applyPreset = useCallback(
    (presetName: string) => {
      const preset = EQ_PRESETS.find((p) => p.name === presetName);
      if (!preset) return;
      for (let i = 0; i < preset.gains.length; i++) {
        dispatchSetEqBand(selectedChannel, i, preset.gains[i]);
      }
    },
    [dispatchSetEqBand, selectedChannel]
  );

  const currentPresetName = useMemo(() => {
    for (const preset of EQ_PRESETS) {
      const match = preset.gains.every(
        (val, i) => Math.abs((channelGains[i] ?? 0) - val) < 0.15
      );
      if (match) return preset.name;
    }
    return "Custom";
  }, [channelGains]);

  // Compute exact node coordinates
  // Range: 85 to 914.8 on x-axis (step ~92.2)
  // Baseline 0 dB is at y = 160. +12 dB is at y = 40. -12 dB is at y = 280.
  const nodes: Point[] = FREQ_LABELS.map((_, i) => {
    const x = 85 + i * BAND_SPACING;
    const gain = channelGains[i] ?? 0;
    const y = Y_BASELINE - gain * 10;
    return { x, y };
  });

  // Construct boundary roll-off anchors so the curve naturally starts and ends at 0 dB baseline
  const g0 = channelGains[0] ?? 0;
  const g9 = channelGains[9] ?? 0;
  const splinePoints: Point[] = [
    { x: 15, y: Y_BASELINE },
    { x: 50, y: Y_BASELINE - g0 * 0.45 * 10 },
    ...nodes,
    { x: 950, y: Y_BASELINE - g9 * 0.45 * 10 },
    { x: 985, y: Y_BASELINE },
  ];

  const { curvePath, fillPath } = getMonotoneSplinePaths(splinePoints);

  const handlePointerDown = (index: number, e: React.PointerEvent) => {
    setDraggingIndex(index);
    (e.target as Element).setPointerCapture(e.pointerId);
  };

  const handlePointerMove = (e: React.PointerEvent) => {
    if (draggingIndex === null || !svgRef.current) return;
    const svg = svgRef.current;
    const ctm = svg.getScreenCTM();
    if (!ctm) return;

    // Use exact SVG CTM matrix inverse for 100% responsive point mapping
    const pt = svg.createSVGPoint();
    pt.x = e.clientX;
    pt.y = e.clientY;
    const svgP = pt.matrixTransform(ctm.inverse());

    // Map svgP.y (40..280) to gain (+12..-12)
    const rawGain = (160 - svgP.y) / 10;
    const gain = Math.max(-12, Math.min(12, Math.round(rawGain * 10) / 10));
    dispatchSetEqBand(selectedChannel, draggingIndex, gain);
  };

  const handlePointerUp = (e: React.PointerEvent) => {
    if (draggingIndex !== null) {
      try {
        (e.target as Element).releasePointerCapture(e.pointerId);
      } catch {}
      setDraggingIndex(null);
    }
  };

  return (
    <div className="flex flex-col gap-6 w-full max-w-7xl mx-auto pb-8 select-none">
      {/* Top Header: Title and Channel Selector Pill */}
      <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-4">
        <div>
          <h1 className="text-xl font-bold tracking-tight text-white font-sans">
            Tuning
          </h1>
        </div>

        {/* Channel Segmented Pill Selector */}
        <div className="flex flex-wrap items-center gap-1 rounded-full border-2 border-white/10 bg-[#202026] p-1.5 self-start sm:self-auto">
          {CHANNELS.map((ch) => {
            const Icon = ch.icon;
            const isSelected = ch.id === selectedChannel;
            return (
              <button
                type="button"
                key={ch.id}
                onClick={() => setSelectedChannel(ch.id)}
                className={`flex items-center gap-2 rounded-full px-4 py-2 text-xs font-semibold transition-colors cursor-pointer border-2 ${
                  isSelected
                    ? "bg-[#ff5511] border-[#ff5511] text-white"
                    : "border-transparent text-zinc-300 hover:text-white hover:bg-[#282830]"
                }`}
              >
                <Icon className={`h-3.5 w-3.5 ${isSelected ? "text-white" : "text-zinc-400"}`} />
                <span>{ch.name}</span>
              </button>
            );
          })}
        </div>
      </div>

      {/* Main Equalizer Card */}
      <div className="flex flex-col rounded-3xl border-2 border-white/10 bg-[#202026] p-4 sm:p-6">
        {/* Equalizer Header Row */}
        <div className="flex flex-wrap items-center justify-between gap-3 pb-4">
          <div className="flex items-center gap-3">
            <h2 className="text-base font-bold text-white tracking-tight">
              Equalizer ({activeChannel.name})
            </h2>
            {currentPresetName !== "Custom" && (
              <span className="rounded-full border border-[#ff5511]/30 bg-[#ff5511]/10 px-2.5 py-0.5 text-[11px] font-mono font-medium text-[#ff5511]">
                {currentPresetName}
              </span>
            )}
          </div>

          <div className="flex flex-wrap items-center gap-2">
            {/* Presets Selector Dropdown */}
            <div className="flex items-center gap-2 rounded-xl border-2 border-white/10 bg-[#282830] px-3 py-1.5 text-xs">
              <Sliders className="h-3.5 w-3.5 text-zinc-400" />
              <span className="text-zinc-400 font-mono text-[11px]">Preset:</span>
              <select
                value={currentPresetName}
                onChange={(e) => applyPreset(e.target.value)}
                className="bg-transparent text-white font-medium focus:outline-none cursor-pointer text-xs"
              >
                <option value="Custom" className="bg-[#202026] text-white">
                  Custom
                </option>
                {EQ_PRESETS.map((p) => (
                  <option key={p.name} value={p.name} className="bg-[#202026] text-white">
                    {p.name}
                  </option>
                ))}
              </select>
            </div>

            {/* Reset flat button */}
            <button
              type="button"
              onClick={handleResetFlat}
              className="flex items-center gap-2 rounded-xl border-2 border-white/10 bg-[#282830] hover:bg-[#32323c] hover:border-white/20 px-3.5 py-2 text-xs font-medium text-zinc-200 hover:text-white transition-colors cursor-pointer"
            >
              <RotateCcw className="h-3.5 w-3.5" />
              <span>Reset flat</span>
            </button>
          </div>
        </div>

        {/* Interactive Responsive SVG EQ Curve */}
        <div className="relative w-full overflow-hidden rounded-2xl bg-[#16161b] border-2 border-white/10 pt-4 pb-2">
          <svg
            ref={svgRef}
            viewBox="0 0 1000 320"
            preserveAspectRatio="xMidYMid meet"
            className="w-full h-auto aspect-[1000/320] max-h-[340px] select-none touch-none block"
            onPointerMove={handlePointerMove}
            onPointerUp={handlePointerUp}
          >
            <defs>
              <linearGradient id="eqFillGradient" x1="0" y1="0" x2="0" y2="1">
                <stop offset="0%" stopColor="#ff5511" stopOpacity="0.45" />
                <stop offset="50%" stopColor="#ff5511" stopOpacity="0.10" />
                <stop offset="100%" stopColor="#ff5511" stopOpacity="0.35" />
              </linearGradient>
            </defs>

            {/* Horizontal Dashed Grid Lines and dB Labels */}
            {[
              { db: "+12", y: 40 },
              { db: "+6", y: 100 },
              { db: "0", y: 160 },
              { db: "-6", y: 220 },
              { db: "-12", y: 280 },
            ].map((line) => (
              <g key={line.db}>
                <text
                  x="48"
                  y={line.y + 4}
                  textAnchor="end"
                  className="text-[11px] font-mono fill-zinc-400 font-medium select-none"
                >
                  {line.db}
                </text>
                <line
                  x1="65"
                  y1={line.y}
                  x2="945"
                  y2={line.y}
                  stroke={line.db === "0" ? "rgba(255,255,255,0.22)" : "rgba(255,255,255,0.06)"}
                  strokeDasharray={line.db === "0" ? "none" : "4 4"}
                  strokeWidth="1"
                />
              </g>
            ))}

            {/* Hover Vertical Guide Beam */}
            {hoveredBand !== null && (
              <rect
                x={nodes[hoveredBand].x - 42}
                y="30"
                width="84"
                height="260"
                fill="rgba(255, 85, 17, 0.05)"
                rx="8"
                className="pointer-events-none"
              />
            )}

            {/* Area Fill between Curve and 0 dB baseline */}
            {fillPath && (
              <path
                d={fillPath}
                fill="url(#eqFillGradient)"
                className="pointer-events-none"
              />
            )}

            {/* Vertical Stems from 0dB baseline to each Node */}
            {nodes.map((node, i) => (
              <line
                key={`stem-${i}`}
                x1={node.x}
                y1={Y_BASELINE}
                x2={node.x}
                y2={node.y}
                stroke={hoveredBand === i ? "rgba(255, 85, 17, 0.9)" : "rgba(255, 85, 17, 0.45)"}
                strokeWidth={hoveredBand === i ? "2" : "1.5"}
                className="pointer-events-none transition-all"
              />
            ))}

            {/* Precision Monotone Bell Spline Curve */}
            {curvePath && (
              <path
                d={curvePath}
                fill="none"
                stroke="#ff5511"
                strokeWidth="3.5"
                strokeLinecap="round"
                strokeLinejoin="round"
                className="pointer-events-none filter drop-shadow-[0_0_8px_rgba(255,85,17,0.45)]"
              />
            )}

            {/* 10 Draggable Band Nodes - Circular Handles */}
            {nodes.map((node, i) => {
              const gain = channelGains[i] ?? 0;
              const isNonZero = Math.abs(gain) > 0.05;
              const isDragging = draggingIndex === i;
              const isHovered = hoveredBand === i;

              return (
                <g key={`node-${i}`}>
                  {/* Outer glow ring on active drag/hover */}
                  {(isDragging || isHovered) && (
                    <circle
                      cx={node.x}
                      cy={node.y}
                      r="13"
                      fill="rgba(255, 85, 17, 0.22)"
                      className="pointer-events-none transition-all"
                    />
                  )}

                  {/* Center Solid Circle Node */}
                  <circle
                    cx={node.x}
                    cy={node.y}
                    r={isDragging ? 7.5 : isHovered ? 7 : 6}
                    fill="#ff5511"
                    stroke="#ffffff"
                    strokeWidth="1.8"
                    onPointerDown={(e) => handlePointerDown(i, e)}
                    onDoubleClick={() => dispatchSetEqBand(selectedChannel, i, 0.0)}
                    onMouseEnter={() => setHoveredBand(i)}
                    onMouseLeave={() => setHoveredBand(null)}
                    onWheel={(e) => {
                      e.preventDefault();
                      const delta = e.deltaY < 0 ? 0.5 : -0.5;
                      const next = Math.max(-12, Math.min(12, Math.round((gain + delta) * 10) / 10));
                      dispatchSetEqBand(selectedChannel, i, next);
                    }}
                    className="cursor-ns-resize transition-all drop-shadow-[0_0_6px_rgba(255,85,17,0.6)]"
                  >
                    <title>{`${FREQ_LABELS[i]}Hz: ${gain > 0 ? "+" : ""}${gain.toFixed(1)} dB (drag, scroll wheel, or double-click to reset)`}</title>
                  </circle>

                  {/* Numerical Gain Badge */}
                  {isNonZero && (
                    <text
                      x={node.x}
                      y={node.y > 60 ? node.y - 12 : node.y + 20}
                      textAnchor="middle"
                      className="text-[10px] font-mono font-bold fill-white select-none pointer-events-none drop-shadow-[0_1px_2px_rgba(0,0,0,0.9)]"
                    >
                      {`${gain > 0 ? "+" : ""}${gain.toFixed(1)}`}
                    </text>
                  )}
                </g>
              );
            })}

            {/* Frequency Labels along bottom */}
            {nodes.map((node, i) => (
              <text
                key={`label-${i}`}
                x={node.x}
                y="308"
                textAnchor="middle"
                className={`text-[11px] font-mono font-medium select-none transition-colors ${
                  hoveredBand === i ? "fill-[#ff5511] font-bold" : "fill-zinc-400"
                }`}
              >
                {FREQ_LABELS[i]}
              </text>
            ))}
          </svg>

          {/* Bottom Card Specs & Interactive Hint */}
          <div className="flex flex-col sm:flex-row sm:items-center justify-between px-6 pt-2 pb-1 text-[11px] font-mono text-zinc-400 border-t border-white/5 gap-1">
            <span>10 bands · Q ≈ 1.41 · ±12 dB peaking filters</span>
            <div className="flex items-center gap-3">
              <span className="hidden sm:inline text-zinc-500">Scroll wheel on node to adjust</span>
              <span>48 kHz 32-bit float</span>
            </div>
          </div>
        </div>
      </div>

      {/* Effects Section */}
      <div className="flex flex-col gap-3">
        <div>
          <h2 className="text-base font-bold text-white tracking-tight">
            Effects
          </h2>
        </div>

        {/* Modular Effects Grid */}
        <div className="grid grid-cols-1 lg:grid-cols-2 xl:grid-cols-3 gap-4">
          {/* AI Noise Suppression (RNNoise) */}
          <div
            className={`flex flex-col justify-between rounded-3xl border-2 p-5 gap-4 transition-colors ${
              isAiNoiseActive
                ? "border-[#ff5511]/50 bg-[#24242c]"
                : "border-white/10 bg-[#202026]"
            }`}
          >
            <div className="flex items-start justify-between gap-3">
              <div className="flex items-center gap-3">
                <div
                  className={`flex h-10 w-10 items-center justify-center rounded-xl border-2 transition-colors ${
                    isAiNoiseActive
                      ? "bg-[#ff5511]/20 border-[#ff5511]/40 text-[#ff5511]"
                      : "bg-[#16161b] border-white/10 text-zinc-300"
                  }`}
                >
                  <AudioWaveform className="h-5 w-5" />
                </div>
                <div>
                  <h3 className="text-sm font-bold text-white tracking-tight">
                    AI Noise Suppression
                  </h3>
                  <span className="text-[11px] font-mono text-zinc-400">RNNoise Neural Filter</span>
                </div>
              </div>

              {/* Toggle Switch */}
              <button
                type="button"
                onClick={() => toggleAiNoise(selectedChannel, !isAiNoiseActive)}
                disabled={!isConnected}
                className={`relative inline-flex h-6 w-11 shrink-0 items-center rounded-full transition-colors duration-200 cursor-pointer disabled:opacity-40 disabled:cursor-not-allowed ${
                  isAiNoiseActive ? "bg-[#ff5511]" : "bg-[#2c2c36]"
                }`}
                aria-label="Toggle AI Noise Suppression"
              >
                <span
                  className={`inline-block h-4 w-4 transform rounded-full bg-white transition-transform duration-200 ${
                    isAiNoiseActive ? "translate-x-5.5" : "translate-x-1"
                  }`}
                />
              </button>
            </div>

            <p className="text-xs text-zinc-300 leading-relaxed">
              Filters background chatter, fan whine, and keyboard noise using neural networks.
            </p>
          </div>
        </div>
      </div>
    </div>
  );
}
