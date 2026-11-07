import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";
import {
  Check,
  Copy,
  Cpu,
  Server,
} from "lucide-react";
import { useAudioStore } from "../store/useAudioStore";

export function AboutView() {
  const status = useAudioStore((s) => s.connectionStatus);
  const latency = useAudioStore((s) => s.latencyMs);
  const isAudioActive = useAudioStore((s) => s.isAudioActive);
  const peakDb = useAudioStore((s) => s.peakDb);
  const rmsDb = useAudioStore((s) => s.rmsDb);
  const seq = useAudioStore((s) => s.sequenceNumber);
  const activeApps = useAudioStore((s) => s.activeApps);
  const hardwareOutputs = useAudioStore((s) => s.hardwareOutputs);

  const [copied, setCopied] = useState(false);
  const [version, setVersion] = useState<string>(
    import.meta.env.VITE_APP_VERSION || "dev"
  );

  useEffect(() => {
    getVersion()
      .then((v) => {
        if (v) setVersion(v);
      })
      .catch(() => {
        // Web preview or dev fallback
      });
  }, []);

  const displayVersion =
    version === "0.0.0-dev" || version === "dev"
      ? "dev"
      : version.startsWith("v")
        ? version
        : `v${version}`;

  const handleCopyDiagnostics = () => {
    const report = [
      "=== Routune Studio Diagnostic Report ===",
      `Version: ${displayVersion} (Desktop Linux x86_64)`,
      "GUI Framework: Tauri v2 / React 18 / Tailwind CSS",
      `Engine Status: ${status.toUpperCase()}`,
      `IPC Latency: ${status === "connected" ? `${latency} ms` : "N/A"}`,
      "PipeWire Audio Backend: 48 kHz / 32-bit float",
      `Active Streams / Apps: ${activeApps.length}`,
      `Hardware Outputs: ${hardwareOutputs.length}`,
      `Peak / RMS Level: ${status === "connected" ? `${peakDb.toFixed(1)} dB / ${rmsDb.toFixed(1)} dB` : "-∞ dB"}`,
      `Stream Activity: ${isAudioActive ? "ACTIVE" : "STANDBY"}`,
      `Heartbeat Sequence: ${seq}`,
      "License: GPL-3.0-only",
      "========================================",
    ].join("\n");

    navigator.clipboard.writeText(report).then(() => {
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    });
  };

  return (
    <div className="flex flex-col items-center justify-center w-full max-w-2xl mx-auto py-2 sm:py-4 select-none">
      {/* Native Desktop App Card */}
      <div className="w-full flex flex-col items-center rounded-3xl border-2 border-[#2B2942] bg-[#171528] p-6 sm:p-8 shadow-2xl shadow-black/40">
        {/* App Hero / Icon Squircle */}
        <div className="flex flex-col items-center text-center gap-3 mb-6">
          <div className="relative flex h-24 w-24 items-center justify-center rounded-3xl bg-gradient-to-br from-[#7C6AF2]/25 to-[#7C6AF2]/5 border-2 border-[#7C6AF2]/40 text-[#7C6AF2] shadow-xl shadow-[#7C6AF2]/20 p-2">
            <img src="/logo.svg" alt="Routune" className="h-full w-full object-contain pointer-events-none" />
            <div className="absolute -bottom-1 -right-1 flex h-6 w-6 items-center justify-center rounded-full bg-[#171528] border-2 border-[#2B2942]">
              <span className="relative flex h-2.5 w-2.5">
                {status === "connected" ? (
                  <>
                    <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-emerald-400 opacity-75" />
                    <span className="relative inline-flex rounded-full h-2.5 w-2.5 bg-emerald-500" />
                  </>
                ) : status === "connecting" ? (
                  <span className="relative inline-flex rounded-full h-2.5 w-2.5 bg-amber-400 animate-pulse" />
                ) : (
                  <span className="relative inline-flex rounded-full h-2.5 w-2.5 bg-rose-500" />
                )}
              </span>
            </div>
          </div>

          <div className="flex flex-col items-center gap-1">
            <h1 className="text-2xl font-bold tracking-tight text-[#ECEAFB] font-sans">
              Routune Studio
            </h1>
            <div className="flex items-center gap-2">
              <span className="rounded-full border border-[#7C6AF2]/40 bg-[#7C6AF2]/10 px-3 py-0.5 text-xs font-mono font-semibold text-[#C4B5FD]">
                {displayVersion}
              </span>
              <span className="rounded-full border border-[#2B2942] bg-white/5 px-2.5 py-0.5 text-[11px] font-mono text-[#ECEAFB]/60">
                Linux (x86_64)
              </span>
            </div>
          </div>
        </div>

        {/* Grouped Desktop Inset Lists */}
        <div className="flex flex-col gap-4 w-full">
          {/* Audio Engine Inset Group */}
          <div className="flex flex-col rounded-2xl border-2 border-[#2B2942] bg-[#0F0E17] overflow-hidden">
            <div className="flex items-center justify-between px-4 py-2.5 border-b border-[#2B2942] bg-white/[0.02]">
              <div className="flex items-center gap-2 text-xs font-bold text-[#ECEAFB] uppercase tracking-wider font-mono">
                <Server className="h-3.5 w-3.5 text-[#7C6AF2]" />
                <span>Audio Engine</span>
              </div>
              <div className="flex items-center gap-1.5 text-[11px] font-mono">
                <span className="text-[#ECEAFB]/50">DAEMON:</span>
                <span
                  className={
                    status === "connected"
                      ? "text-emerald-400 font-semibold"
                      : status === "connecting"
                        ? "text-amber-400 font-semibold"
                        : "text-rose-400 font-semibold"
                  }
                >
                  {status === "connected" ? "CONNECTED" : status === "connecting" ? "CONNECTING" : "OFFLINE"}
                </span>
              </div>
            </div>

            <div className="divide-y divide-[#2B2942]/60 text-xs font-mono">
              <div className="flex items-center justify-between px-4 py-2.5">
                <span className="text-[#ECEAFB]/60">PipeWire Server</span>
                <span className="text-[#ECEAFB] font-medium">1.0+ (Pulse / JACK / ALSA)</span>
              </div>
              <div className="flex items-center justify-between px-4 py-2.5">
                <span className="text-[#ECEAFB]/60">DSP Processing</span>
                <span className="text-[#ECEAFB] font-medium">48 kHz · 32-bit Float · 10-Band EQ</span>
              </div>
              <div className="flex items-center justify-between px-4 py-2.5">
                <span className="text-[#ECEAFB]/60">IPC Telemetry Latency</span>
                <span className="text-[#C4B5FD] font-semibold">
                  {status === "connected" ? `${latency} ms` : "---"}
                </span>
              </div>
              <div className="flex items-center justify-between px-4 py-2.5">
                <span className="text-[#ECEAFB]/60">Active Levels (Peak / RMS)</span>
                <span className="text-[#ECEAFB] font-medium">
                  {status === "connected" ? `${peakDb.toFixed(1)} dB / ${rmsDb.toFixed(1)} dB` : "-∞ dB"}
                </span>
              </div>
            </div>
          </div>

          {/* System & Architecture Inset Group */}
          <div className="flex flex-col rounded-2xl border-2 border-[#2B2942] bg-[#0F0E17] overflow-hidden">
            <div className="flex items-center justify-between px-4 py-2.5 border-b border-[#2B2942] bg-white/[0.02]">
              <div className="flex items-center gap-2 text-xs font-bold text-[#ECEAFB] uppercase tracking-wider font-mono">
                <Cpu className="h-3.5 w-3.5 text-[#7C6AF2]" />
                <span>Architecture</span>
              </div>
              <div className="flex items-center gap-1.5 text-[11px] font-mono">
                <span className="text-[#ECEAFB]/50">LICENSE:</span>
                <span className="text-[#ECEAFB]/80 font-medium">GPL-3.0</span>
              </div>
            </div>

            <div className="divide-y divide-[#2B2942]/60 text-xs font-mono">
              <div className="flex items-center justify-between px-4 py-2.5">
                <span className="text-[#ECEAFB]/60">Desktop Runtime</span>
                <span className="text-[#ECEAFB] font-medium">Tauri v2 · Rust 2021 Edition</span>
              </div>
              <div className="flex items-center justify-between px-4 py-2.5">
                <span className="text-[#ECEAFB]/60">Virtual Channels</span>
                <span className="text-[#ECEAFB] font-medium">Master, Media, Game, Chat, Aux, Mic</span>
              </div>
              <div className="flex items-center justify-between px-4 py-2.5">
                <span className="text-[#ECEAFB]/60">Active Audio Streams</span>
                <span className="text-emerald-400 font-semibold">{activeApps.length} connected</span>
              </div>
              <div className="flex items-center justify-between px-4 py-2.5">
                <span className="text-[#ECEAFB]/60">Hardware Output Sinks</span>
                <span className="text-[#ECEAFB] font-medium">{hardwareOutputs.length} available</span>
              </div>
            </div>
          </div>
        </div>

        {/* Action Controls & Footer */}
        <div className="flex flex-col sm:flex-row items-center justify-between gap-3 w-full mt-6">
          <button
            type="button"
            onClick={handleCopyDiagnostics}
            className="w-full sm:w-auto flex items-center justify-center gap-2 rounded-xl border-2 border-[#2B2942] bg-[#0F0E17] hover:bg-[#201D36] hover:border-[#3F3B5F] px-4 py-2 text-xs font-semibold text-[#ECEAFB]/80 hover:text-white transition-all cursor-pointer"
            title="Copy system and audio engine diagnostics to clipboard"
          >
            {copied ? (
              <>
                <Check className="h-3.5 w-3.5 text-emerald-400" />
                <span className="text-emerald-300">Diagnostics Copied</span>
              </>
            ) : (
              <>
                <Copy className="h-3.5 w-3.5 text-[#ECEAFB]/60" />
                <span>Copy Diagnostics</span>
              </>
            )}
          </button>

          <span className="text-[11px] text-[#ECEAFB]/50 font-mono text-center sm:text-right">
            Built with Rust, Tauri & PipeWire
          </span>
        </div>
      </div>
    </div>
  );
}
