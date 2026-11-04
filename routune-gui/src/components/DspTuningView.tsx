import { useState, useRef, useCallback, useMemo, useEffect } from "react";
import {
  Activity,
  Gamepad2,
  MessageSquare,
  Mic,
  Music,
  Radio,
  RotateCcw,
  AudioWaveform,
  Sliders,
  Headphones,
  Loader2,
  Volume2,
  Speaker,
} from "lucide-react";
import { useAudioStore } from "../store/useAudioStore";
import { AudioChannel, SpatialAudioMode } from "../proto/routune";
import { routuneClient } from "../services/grpcClient";

interface ChannelTab {
  id: AudioChannel;
  name: string;
  icon: typeof Gamepad2;
}

const CHANNELS: ChannelTab[] = [
  { id: AudioChannel.MIC, name: "Mic", icon: Mic },
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
  const aiNoiseLevel = useAudioStore((s) => s.aiNoiseLevel);
  const setAiNoiseLevel = useAudioStore((s) => s.setAiNoiseLevel);
  const noiseGate = useAudioStore((s) => s.noiseGate);
  const compressor = useAudioStore((s) => s.compressor);
  const setNoiseGate = useAudioStore((s) => s.setNoiseGate);
  const setCompressor = useAudioStore((s) => s.setCompressor);
  const spatialAudio = useAudioStore((s) => s.spatialAudio);
  const volumeBoost = useAudioStore((s) => s.volumeBoost);
  const setSpatialAudio = useAudioStore((s) => s.setSpatialAudio);
  const setVolumeBoost = useAudioStore((s) => s.setVolumeBoost);
  const micTestEnabled = useAudioStore((s) => s.micTestEnabled);
  const isTogglingMicTest = useAudioStore((s) => s.isTogglingMicTest);
  const toggleMicTest = useAudioStore((s) => s.toggleMicTest);
  const fetchMicTest = useAudioStore((s) => s.fetchMicTest);

  useEffect(() => {
    if (isConnected) {
      fetchMicTest();
    }
  }, [isConnected, fetchMicTest]);

  const activeChannel = CHANNELS.find((c) => c.id === selectedChannel) || CHANNELS[0];
  const channelGains = eqBands[selectedChannel] || [0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
  const isAiNoiseActive = channelNoiseEnabled[selectedChannel] ?? false;

  const isPlaybackDspChannel =
    selectedChannel === AudioChannel.GAME ||
    selectedChannel === AudioChannel.MEDIA ||
    selectedChannel === AudioChannel.AUX;

  const currentSpatial = spatialAudio[selectedChannel] ?? {
    enabled: false,
    mode: SpatialAudioMode.HEADPHONE,
    strength: 1.0,
  };

  const currentBoost = volumeBoost[selectedChannel] ?? {
    enabled: false,
    boostDb: 6.0,
  };

  // Single slider compressor amount (0.0 to 1.0) mapped to threshold/ratio/makeup
  const compAmount = Math.max(0, Math.min(1, (-compressor.thresholdDb - 10) / 22));

  const handleCompressorAmountChange = useCallback(
    (amt: number) => {
      const clamped = Math.max(0, Math.min(1, amt));
      const thresholdDb = -10.0 - clamped * 22.0;
      const ratio = 2.0 + clamped * 4.0;
      const makeupGainDb = clamped * 4.0;
      setCompressor({
        thresholdDb,
        ratio,
        makeupGainDb,
        attackMs: 10.0,
        releaseMs: 100.0,
      });
    },
    [setCompressor]
  );

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
  // Anchors start at x: 65 and end at x: 945 to match grid line bounds and avoid overlapping Y-axis dB labels (x <= 48)
  const splinePoints: Point[] = [
    { x: 65, y: Y_BASELINE },
    ...nodes,
    { x: 945, y: Y_BASELINE },
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
      <div className="flex flex-col lg:flex-row lg:items-center justify-between gap-3 sm:gap-4">
        <div className="flex items-center justify-between w-full lg:w-auto">
          <h1 className="text-xl font-bold tracking-tight text-white font-sans">
            Tuning
          </h1>

          {/* On compact / minimum window: Test Mic button right-aligned on title row */}
          {selectedChannel === AudioChannel.MIC && (
            <button
              type="button"
              disabled={isTogglingMicTest}
              onClick={() => toggleMicTest()}
              className={`lg:hidden flex items-center gap-2 rounded-full border-2 px-3.5 py-1.5 text-xs font-semibold transition-all cursor-pointer shrink-0 ${
                micTestEnabled
                  ? "border-emerald-500/60 bg-emerald-500/20 text-emerald-300 shadow-md shadow-emerald-950/40"
                  : "border-white/10 bg-[#202026] text-zinc-300 hover:text-white hover:border-white/20"
              } disabled:opacity-50`}
              title="Listen to microphone with live DSP effects directly in headphones"
            >
              {isTogglingMicTest ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin text-emerald-400" />
              ) : (
                <Headphones className={`h-3.5 w-3.5 shrink-0 ${micTestEnabled ? "text-emerald-400" : "text-zinc-400"}`} />
              )}
              <span className="whitespace-nowrap">{micTestEnabled ? "Testing Mic" : "Test Mic"}</span>
            </button>
          )}
        </div>

        {/* Channel Segmented Pill Selector and Mic Test Button (on lg+) */}
        <div className="flex items-center gap-2 self-start lg:self-auto w-full lg:w-auto overflow-x-auto scrollbar-none">
          {selectedChannel === AudioChannel.MIC && (
            <button
              type="button"
              disabled={isTogglingMicTest}
              onClick={() => toggleMicTest()}
              className={`hidden lg:flex items-center gap-2 rounded-full border-2 px-4 py-2 text-xs font-semibold transition-all cursor-pointer shrink-0 ${
                micTestEnabled
                  ? "border-emerald-500/60 bg-emerald-500/20 text-emerald-300 shadow-lg shadow-emerald-950/40"
                  : "border-white/10 bg-[#202026] text-zinc-300 hover:text-white hover:border-white/20"
              } disabled:opacity-50`}
              title="Listen to microphone with live DSP effects directly in headphones"
            >
              {isTogglingMicTest ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin text-emerald-400" />
              ) : (
                <Headphones className={`h-3.5 w-3.5 shrink-0 ${micTestEnabled ? "text-emerald-400" : "text-zinc-400"}`} />
              )}
              <span className="whitespace-nowrap">{micTestEnabled ? "Testing Mic (Active)" : "Test Mic"}</span>
            </button>
          )}

          <div className="flex items-center gap-1 rounded-full border-2 border-white/10 bg-[#202026] p-1.5 shrink-0">
            {CHANNELS.map((ch) => {
              const Icon = ch.icon;
              const isSelected = ch.id === selectedChannel;
              return (
                <button
                  type="button"
                  key={ch.id}
                  onClick={() => setSelectedChannel(ch.id)}
                  className={`flex items-center gap-2 rounded-full px-4 py-2 text-xs font-semibold transition-colors cursor-pointer border-2 shrink-0 ${
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
      </div>

      {/* Main Equalizer Card */}
      <div className="flex flex-col rounded-3xl border-2 border-white/10 bg-[#202026] p-4 sm:p-6 gap-5">
        {/* Equalizer Header Row */}
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div className="flex items-center gap-3">
            <h2 className="text-base font-bold text-white tracking-tight">
              Equalizer ({activeChannel.name})
            </h2>
            {currentPresetName === "Custom" ? (
              <span className="rounded-full border border-amber-500/30 bg-amber-500/10 px-2.5 py-0.5 text-[11px] font-mono font-medium text-amber-300">
                Custom
              </span>
            ) : (
              <span className="rounded-full border border-[#ff5511]/30 bg-[#ff5511]/10 px-2.5 py-0.5 text-[11px] font-mono font-medium text-[#ff5511]">
                {currentPresetName}
              </span>
            )}
          </div>

          <button
            type="button"
            onClick={handleResetFlat}
            className="flex items-center gap-1.5 rounded-xl border-2 border-white/10 bg-[#282830] hover:bg-[#32323c] hover:border-white/20 px-3.5 py-1.5 text-xs font-medium text-zinc-200 hover:text-white transition-all cursor-pointer"
            title="Reset all 10 bands to 0 dB flat"
          >
            <RotateCcw className="h-3.5 w-3.5 text-zinc-400" />
            <span>Reset flat</span>
          </button>
        </div>

        {/* Quick Presets Grid */}
        <div className="flex flex-col gap-2">
          <span className="text-[10px] font-mono uppercase tracking-widest text-zinc-500">
            Presets
          </span>
          <div className="flex flex-wrap gap-2">
            {EQ_PRESETS.map((p) => {
              const isActive = currentPresetName === p.name;
              return (
                <button
                  key={p.name}
                  type="button"
                  onClick={() => applyPreset(p.name)}
                  className={`flex items-center gap-2 rounded-2xl px-4 py-2 text-xs font-semibold transition-all cursor-pointer border-2 ${
                    isActive
                      ? "border-[#ff5511] bg-[#ff5511] text-white shadow-md shadow-[#ff5511]/20"
                      : "border-transparent bg-[#16161b] text-zinc-400 hover:border-transparent hover:text-white hover:bg-[#282830]"
                  }`}
                >
                  <span>{p.name}</span>
                </button>
              );
            })}
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
          {/* AI Noise Suppression (RNNoise) - Only available for Chat & Mic */}
          {(selectedChannel === AudioChannel.CHAT || selectedChannel === AudioChannel.MIC) && (
            <div
              className={`flex flex-col justify-between rounded-3xl border p-5 gap-4 transition-colors ${
                isAiNoiseActive
                  ? "border-[#ff5511]/50 bg-[#24242c]"
                  : "border-white/10 bg-[#202026]"
              }`}
            >
              <div className="flex items-start justify-between gap-3">
                <div className="flex items-center gap-3">
                  <div
                    className={`flex h-10 w-10 items-center justify-center rounded-xl border transition-colors ${
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
                  role="switch"
                  aria-checked={isAiNoiseActive}
                  onClick={() => toggleAiNoise(selectedChannel, !isAiNoiseActive)}
                  disabled={!isConnected}
                  className={`relative inline-flex h-6 w-11 shrink-0 p-0.5 items-center rounded-full transition-colors duration-200 cursor-pointer disabled:opacity-40 disabled:cursor-not-allowed border-0 ${
                    isAiNoiseActive
                      ? "bg-[#ff5511] shadow-[0_0_12px_rgba(255,85,17,0.4)]"
                      : "bg-[#282832] ring-1 ring-white/10 hover:ring-white/20"
                  }`}
                  aria-label="Toggle AI Noise Suppression"
                >
                  <span
                    className={`inline-block h-5 w-5 transform rounded-full bg-white shadow-md shadow-black/50 transition-transform duration-200 ease-out pointer-events-none ${
                      isAiNoiseActive ? "translate-x-5" : "translate-x-0"
                    }`}
                  />
                </button>
              </div>

              <p className="text-xs text-zinc-300 leading-relaxed">
                Filters background chatter, fan whine, and keyboard noise using neural networks.
              </p>

              {/* Suppression Level Slider */}
              <div className="flex flex-col gap-1.5">
                <div className="flex items-center justify-between text-xs">
                  <span className="text-zinc-400 font-medium">Reduction Level</span>
                  <span className="font-mono text-zinc-200 font-semibold">
                    {Math.round((aiNoiseLevel ?? 1.0) * 100)}%
                  </span>
                </div>
                <input
                  type="range"
                  min="0"
                  max="1"
                  step="0.01"
                  value={aiNoiseLevel ?? 1.0}
                  disabled={!isConnected || !isAiNoiseActive}
                  onChange={(e) => {
                    const val = parseFloat(e.target.value);
                    setAiNoiseLevel(selectedChannel, val);
                  }}
                  className="w-full h-1.5 bg-zinc-700 rounded-lg appearance-none cursor-pointer accent-[#ff5511] disabled:opacity-40 disabled:cursor-not-allowed"
                  title={`AI Noise Level: ${Math.round((aiNoiseLevel ?? 1.0) * 100)}%`}
                />
                <div className="flex justify-between text-[10px] font-mono font-medium text-zinc-500 select-none px-0.5">
                  <button
                    type="button"
                    onClick={() => isAiNoiseActive && setAiNoiseLevel(selectedChannel, 0.0)}
                    disabled={!isConnected || !isAiNoiseActive}
                    className="hover:text-zinc-300 disabled:opacity-40 transition-colors cursor-pointer"
                  >
                    Min
                  </button>
                  <button
                    type="button"
                    onClick={() => isAiNoiseActive && setAiNoiseLevel(selectedChannel, 1.0)}
                    disabled={!isConnected || !isAiNoiseActive}
                    className="hover:text-zinc-300 disabled:opacity-40 transition-colors cursor-pointer"
                  >
                    Max
                  </button>
                </div>
              </div>
            </div>
          )}

          {/* Spatial Audio & Volume Boost for Game, Media, and Aux */}
          {isPlaybackDspChannel && (
            <>
              {/* Spatial Audio Card */}
              <div
                className={`flex flex-col justify-between rounded-3xl border p-5 gap-4 transition-colors ${
                  currentSpatial.enabled
                    ? "border-[#ff5511]/50 bg-[#24242c]"
                    : "border-white/10 bg-[#202026]"
                }`}
              >
                <div className="flex items-start justify-between gap-3">
                  <div className="flex items-center gap-3">
                    <div
                      className={`flex h-10 w-10 items-center justify-center rounded-xl border transition-colors ${
                        currentSpatial.enabled
                          ? "bg-[#ff5511]/20 border-[#ff5511]/40 text-[#ff5511]"
                          : "bg-[#16161b] border-white/10 text-zinc-300"
                      }`}
                    >
                      <Headphones className="h-5 w-5" />
                    </div>
                    <div>
                      <h3 className="text-sm font-bold text-white tracking-tight">Spatial Audio</h3>
                      <span className="text-[11px] font-mono text-zinc-400">3D Soundstage</span>
                    </div>
                  </div>

                  {/* Toggle Switch */}
                  <button
                    type="button"
                    role="switch"
                    aria-checked={currentSpatial.enabled}
                    onClick={() =>
                      setSpatialAudio(selectedChannel, { enabled: !currentSpatial.enabled })
                    }
                    disabled={!isConnected}
                    className={`relative inline-flex h-6 w-11 shrink-0 p-0.5 items-center rounded-full transition-colors duration-200 cursor-pointer disabled:opacity-40 disabled:cursor-not-allowed border-0 ${
                      currentSpatial.enabled
                        ? "bg-[#ff5511] shadow-[0_0_12px_rgba(255,85,17,0.4)]"
                        : "bg-[#282832] ring-1 ring-white/10 hover:ring-white/20"
                    }`}
                    aria-label="Toggle Spatial Audio"
                  >
                    <span
                      className={`inline-block h-5 w-5 transform rounded-full bg-white shadow-md shadow-black/50 transition-transform duration-200 ease-out pointer-events-none ${
                        currentSpatial.enabled ? "translate-x-5" : "translate-x-0"
                      }`}
                    />
                  </button>
                </div>

                {/* Mode Selector Tabs (Headphone vs Speaker) */}
                <div className="flex items-center rounded-xl bg-[#18181e] p-1 border border-white/5 gap-1">
                  <button
                    type="button"
                    onClick={() =>
                      setSpatialAudio(selectedChannel, { mode: SpatialAudioMode.HEADPHONE })
                    }
                    disabled={!isConnected || !currentSpatial.enabled}
                    className={`flex-1 flex items-center justify-center gap-1.5 py-1.5 px-3 rounded-lg text-xs font-medium transition-all ${
                      currentSpatial.mode === SpatialAudioMode.HEADPHONE
                        ? "bg-[#2e2e38] text-white shadow-sm font-semibold border border-white/10"
                        : "text-zinc-400 hover:text-zinc-200 hover:bg-white/[0.03]"
                    } disabled:opacity-40 disabled:cursor-not-allowed cursor-pointer`}
                  >
                    <Headphones className="h-3.5 w-3.5" />
                    Headphone
                  </button>
                  <button
                    type="button"
                    onClick={() =>
                      setSpatialAudio(selectedChannel, { mode: SpatialAudioMode.SPEAKER })
                    }
                    disabled={!isConnected || !currentSpatial.enabled}
                    className={`flex-1 flex items-center justify-center gap-1.5 py-1.5 px-3 rounded-lg text-xs font-medium transition-all ${
                      currentSpatial.mode === SpatialAudioMode.SPEAKER
                        ? "bg-[#2e2e38] text-white shadow-sm font-semibold border border-white/10"
                        : "text-zinc-400 hover:text-zinc-200 hover:bg-white/[0.03]"
                    } disabled:opacity-40 disabled:cursor-not-allowed cursor-pointer`}
                  >
                    <Speaker className="h-3.5 w-3.5" />
                    Speaker
                  </button>
                </div>

                <p className="text-xs text-zinc-300 leading-relaxed">
                  {currentSpatial.mode === SpatialAudioMode.HEADPHONE
                    ? "Binaural crossfeed and HRTF head-shadow delay for immersive headphone immersion."
                    : "Acoustic crosstalk cancellation and stereo soundstage expansion for desktop speakers."}
                </p>

                {/* Spatial Strength Slider */}
                <div className="flex flex-col gap-1.5">
                  <div className="flex items-center justify-between text-xs">
                    <span className="text-zinc-400 font-medium">Effect Strength</span>
                    <span className="font-mono text-zinc-200 font-semibold">
                      {Math.round(currentSpatial.strength * 100)}%
                    </span>
                  </div>
                  <input
                    type="range"
                    min="0"
                    max="1"
                    step="0.01"
                    value={currentSpatial.strength}
                    disabled={!isConnected || !currentSpatial.enabled}
                    onChange={(e) =>
                      setSpatialAudio(selectedChannel, { strength: parseFloat(e.target.value) })
                    }
                    className="w-full h-1.5 bg-zinc-700 rounded-lg appearance-none cursor-pointer accent-[#ff5511] disabled:opacity-40 disabled:cursor-not-allowed"
                    title={`Spatial Strength: ${Math.round(currentSpatial.strength * 100)}%`}
                  />
                  <div className="flex justify-between text-[10px] font-mono font-medium text-zinc-500 select-none px-0.5">
                    <button
                      type="button"
                      onClick={() =>
                        currentSpatial.enabled && setSpatialAudio(selectedChannel, { strength: 0.0 })
                      }
                      disabled={!isConnected || !currentSpatial.enabled}
                      className="hover:text-zinc-300 disabled:opacity-40 transition-colors cursor-pointer"
                    >
                      Min
                    </button>
                    <button
                      type="button"
                      onClick={() =>
                        currentSpatial.enabled && setSpatialAudio(selectedChannel, { strength: 1.0 })
                      }
                      disabled={!isConnected || !currentSpatial.enabled}
                      className="hover:text-zinc-300 disabled:opacity-40 transition-colors cursor-pointer"
                    >
                      Max
                    </button>
                  </div>
                </div>
              </div>

              {/* Volume Boost Card */}
              <div
                className={`flex flex-col justify-between rounded-3xl border p-5 gap-4 transition-colors ${
                  currentBoost.enabled
                    ? "border-[#ff5511]/50 bg-[#24242c]"
                    : "border-white/10 bg-[#202026]"
                }`}
              >
                <div className="flex items-start justify-between gap-3">
                  <div className="flex items-center gap-3">
                    <div
                      className={`flex h-10 w-10 items-center justify-center rounded-xl border transition-colors ${
                        currentBoost.enabled
                          ? "bg-[#ff5511]/20 border-[#ff5511]/40 text-[#ff5511]"
                          : "bg-[#16161b] border-white/10 text-zinc-300"
                      }`}
                    >
                      <Volume2 className="h-5 w-5" />
                    </div>
                    <div>
                      <h3 className="text-sm font-bold text-white tracking-tight">Volume Boost</h3>
                      <span className="text-[11px] font-mono text-zinc-400">Soft-Limiting Amp</span>
                    </div>
                  </div>

                  {/* Toggle Switch */}
                  <button
                    type="button"
                    role="switch"
                    aria-checked={currentBoost.enabled}
                    onClick={() =>
                      setVolumeBoost(selectedChannel, { enabled: !currentBoost.enabled })
                    }
                    disabled={!isConnected}
                    className={`relative inline-flex h-6 w-11 shrink-0 p-0.5 items-center rounded-full transition-colors duration-200 cursor-pointer disabled:opacity-40 disabled:cursor-not-allowed border-0 ${
                      currentBoost.enabled
                        ? "bg-[#ff5511] shadow-[0_0_12px_rgba(255,85,17,0.4)]"
                        : "bg-[#282832] ring-1 ring-white/10 hover:ring-white/20"
                    }`}
                    aria-label="Toggle Volume Boost"
                  >
                    <span
                      className={`inline-block h-5 w-5 transform rounded-full bg-white shadow-md shadow-black/50 transition-transform duration-200 ease-out pointer-events-none ${
                        currentBoost.enabled ? "translate-x-5" : "translate-x-0"
                      }`}
                    />
                  </button>
                </div>

                <p className="text-xs text-zinc-300 leading-relaxed">
                  Amplifies quiet audio details with soft saturation limiting to prevent harsh digital clipping.
                </p>

                {/* Boost dB Slider */}
                <div className="flex flex-col gap-1.5">
                  <div className="flex items-center justify-between text-xs">
                    <span className="text-zinc-400 font-medium">Gain Boost</span>
                    <span className="font-mono text-zinc-200 font-semibold">
                      +{currentBoost.boostDb.toFixed(1)} dB
                    </span>
                  </div>
                  <input
                    type="range"
                    min="0"
                    max="12"
                    step="0.5"
                    value={currentBoost.boostDb}
                    disabled={!isConnected || !currentBoost.enabled}
                    onChange={(e) =>
                      setVolumeBoost(selectedChannel, { boostDb: parseFloat(e.target.value) })
                    }
                    className="w-full h-1.5 bg-zinc-700 rounded-lg appearance-none cursor-pointer accent-[#ff5511] disabled:opacity-40 disabled:cursor-not-allowed"
                    title={`Volume Boost: +${currentBoost.boostDb.toFixed(1)} dB`}
                  />
                  <div className="flex justify-between text-[10px] font-mono font-medium text-zinc-500 select-none px-0.5">
                    <button
                      type="button"
                      onClick={() =>
                        currentBoost.enabled && setVolumeBoost(selectedChannel, { boostDb: 0.0 })
                      }
                      disabled={!isConnected || !currentBoost.enabled}
                      className="hover:text-zinc-300 disabled:opacity-40 transition-colors cursor-pointer"
                    >
                      0 dB
                    </button>
                    <button
                      type="button"
                      onClick={() =>
                        currentBoost.enabled && setVolumeBoost(selectedChannel, { boostDb: 12.0 })
                      }
                      disabled={!isConnected || !currentBoost.enabled}
                      className="hover:text-zinc-300 disabled:opacity-40 transition-colors cursor-pointer"
                    >
                      +12 dB
                    </button>
                  </div>
                </div>
              </div>
            </>
          )}

          {/* Fallback info card for channels without additional DSP effects */}
          {!isPlaybackDspChannel && selectedChannel !== AudioChannel.CHAT && selectedChannel !== AudioChannel.MIC && (
            <div className="col-span-full rounded-2xl border border-white/5 bg-[#202026]/60 p-5 text-center text-xs text-zinc-400 font-mono">
              Pure 10-band biquad equalizer active.
            </div>
          )}

          {/* Noise Gate and Compressor ONLY for Mic channel */}
          {selectedChannel === AudioChannel.MIC && (
            <>
              {/* Noise Gate */}
              <div
                className={`flex flex-col justify-between rounded-3xl border p-5 gap-4 transition-colors ${
                  noiseGate.enabled
                    ? "border-[#ff5511]/50 bg-[#24242c]"
                    : "border-white/10 bg-[#202026]"
                }`}
              >
                <div className="flex items-start justify-between gap-3">
                  <div className="flex items-center gap-3">
                    <div
                      className={`flex h-10 w-10 items-center justify-center rounded-xl border transition-colors ${
                        noiseGate.enabled
                          ? "bg-[#ff5511]/20 border-[#ff5511]/40 text-[#ff5511]"
                          : "bg-[#16161b] border-white/10 text-zinc-300"
                      }`}
                    >
                      <Sliders className="h-5 w-5" />
                    </div>
                    <div>
                      <h3 className="text-sm font-bold text-white tracking-tight">Noise Gate</h3>
                      <span className="text-[11px] font-mono text-zinc-400">Background Gate</span>
                    </div>
                  </div>

                  {/* Toggle Switch */}
                  <button
                    type="button"
                    role="switch"
                    aria-checked={noiseGate.enabled}
                    onClick={() => setNoiseGate({ enabled: !noiseGate.enabled })}
                    disabled={!isConnected}
                    className={`relative inline-flex h-6 w-11 shrink-0 p-0.5 items-center rounded-full transition-colors duration-200 cursor-pointer disabled:opacity-40 disabled:cursor-not-allowed border-0 ${
                      noiseGate.enabled
                        ? "bg-[#ff5511] shadow-[0_0_12px_rgba(255,85,17,0.4)]"
                        : "bg-[#282832] ring-1 ring-white/10 hover:ring-white/20"
                    }`}
                    aria-label="Toggle Noise Gate"
                  >
                    <span
                      className={`inline-block h-5 w-5 transform rounded-full bg-white shadow-md shadow-black/50 transition-transform duration-200 ease-out pointer-events-none ${
                        noiseGate.enabled ? "translate-x-5" : "translate-x-0"
                      }`}
                    />
                  </button>
                </div>

                <p className="text-xs text-zinc-300 leading-relaxed">
                  Mutes microphone input when your voice drops below threshold to cut out room hiss.
                </p>

                {/* Single Threshold Slider */}
                <div className="flex flex-col gap-1.5">
                  <div className="flex items-center justify-between text-xs">
                    <span className="text-zinc-400 font-medium">Threshold</span>
                    <span className="font-mono text-zinc-200 font-semibold">
                      {noiseGate.thresholdDb.toFixed(1)} dB
                    </span>
                  </div>
                  <input
                    type="range"
                    min={-70}
                    max={-10}
                    step={0.5}
                    value={noiseGate.thresholdDb}
                    disabled={!isConnected || !noiseGate.enabled}
                    onChange={(e) => setNoiseGate({ thresholdDb: parseFloat(e.target.value) })}
                    className="w-full h-1.5 bg-zinc-700 rounded-lg appearance-none cursor-pointer accent-[#ff5511] disabled:opacity-40 disabled:cursor-not-allowed"
                    title={`Threshold: ${noiseGate.thresholdDb.toFixed(1)} dB`}
                  />
                  <div className="flex justify-between text-[10px] font-mono font-medium text-zinc-500 select-none px-0.5">
                    <button
                      type="button"
                      onClick={() => noiseGate.enabled && setNoiseGate({ thresholdDb: -70 })}
                      disabled={!isConnected || !noiseGate.enabled}
                      className="hover:text-zinc-300 disabled:opacity-40 transition-colors cursor-pointer"
                    >
                      Min
                    </button>
                    <button
                      type="button"
                      onClick={() => noiseGate.enabled && setNoiseGate({ thresholdDb: -10 })}
                      disabled={!isConnected || !noiseGate.enabled}
                      className="hover:text-zinc-300 disabled:opacity-40 transition-colors cursor-pointer"
                    >
                      Max
                    </button>
                  </div>
                </div>
              </div>

              {/* Compressor */}
              <div
                className={`flex flex-col justify-between rounded-3xl border p-5 gap-4 transition-colors ${
                  compressor.enabled
                    ? "border-[#ff5511]/50 bg-[#24242c]"
                    : "border-white/10 bg-[#202026]"
                }`}
              >
                <div className="flex items-start justify-between gap-3">
                  <div className="flex items-center gap-3">
                    <div
                      className={`flex h-10 w-10 items-center justify-center rounded-xl border transition-colors ${
                        compressor.enabled
                          ? "bg-[#ff5511]/20 border-[#ff5511]/40 text-[#ff5511]"
                          : "bg-[#16161b] border-white/10 text-zinc-300"
                      }`}
                    >
                      <Activity className="h-5 w-5" />
                    </div>
                    <div>
                      <h3 className="text-sm font-bold text-white tracking-tight">Compressor</h3>
                      <span className="text-[11px] font-mono text-zinc-400">Dynamic Leveler</span>
                    </div>
                  </div>

                  {/* Toggle Switch */}
                  <button
                    type="button"
                    role="switch"
                    aria-checked={compressor.enabled}
                    onClick={() => setCompressor({ enabled: !compressor.enabled })}
                    disabled={!isConnected}
                    className={`relative inline-flex h-6 w-11 shrink-0 p-0.5 items-center rounded-full transition-colors duration-200 cursor-pointer disabled:opacity-40 disabled:cursor-not-allowed border-0 ${
                      compressor.enabled
                        ? "bg-[#ff5511] shadow-[0_0_12px_rgba(255,85,17,0.4)]"
                        : "bg-[#282832] ring-1 ring-white/10 hover:ring-white/20"
                    }`}
                    aria-label="Toggle Compressor"
                  >
                    <span
                      className={`inline-block h-5 w-5 transform rounded-full bg-white shadow-md shadow-black/50 transition-transform duration-200 ease-out pointer-events-none ${
                        compressor.enabled ? "translate-x-5" : "translate-x-0"
                      }`}
                    />
                  </button>
                </div>

                <p className="text-xs text-zinc-300 leading-relaxed">
                  Evens out loud shouts and quiet whispers to deliver broadcast-quality consistent speech.
                </p>

                {/* Single Compression Amount Slider */}
                <div className="flex flex-col gap-1.5">
                  <div className="flex items-center justify-between text-xs">
                    <span className="text-zinc-400 font-medium">Compression</span>
                    <span className="font-mono text-zinc-200 font-semibold">
                      {Math.round(compAmount * 100)}%
                    </span>
                  </div>
                  <input
                    type="range"
                    min={0}
                    max={1}
                    step="0.01"
                    value={compAmount}
                    disabled={!isConnected || !compressor.enabled}
                    onChange={(e) => handleCompressorAmountChange(parseFloat(e.target.value))}
                    className="w-full h-1.5 bg-zinc-700 rounded-lg appearance-none cursor-pointer accent-[#ff5511] disabled:opacity-40 disabled:cursor-not-allowed"
                    title={`Compression: ${Math.round(compAmount * 100)}%`}
                  />
                  <div className="flex justify-between text-[10px] font-mono font-medium text-zinc-500 select-none px-0.5">
                    <button
                      type="button"
                      onClick={() => compressor.enabled && handleCompressorAmountChange(0.0)}
                      disabled={!isConnected || !compressor.enabled}
                      className="hover:text-zinc-300 disabled:opacity-40 transition-colors cursor-pointer"
                    >
                      Min
                    </button>
                    <button
                      type="button"
                      onClick={() => compressor.enabled && handleCompressorAmountChange(1.0)}
                      disabled={!isConnected || !compressor.enabled}
                      className="hover:text-zinc-300 disabled:opacity-40 transition-colors cursor-pointer"
                    >
                      Max
                    </button>
                  </div>
                </div>
              </div>
            </>
          )}
        </div>
      </div>
    </div>
  );
}
