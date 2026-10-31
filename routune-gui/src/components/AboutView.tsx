import { Activity, AudioWaveform, Info, Radio, Server } from "lucide-react";
import { useAudioStore } from "../store/useAudioStore";

export function AboutView() {
    const status = useAudioStore(s => s.connectionStatus);
    const latency = useAudioStore(s => s.latencyMs);
    const isAudioActive = useAudioStore(s => s.isAudioActive);
    const peakDb = useAudioStore(s => s.peakDb);
    const rmsDb = useAudioStore(s => s.rmsDb);
    const seq = useAudioStore(s => s.sequenceNumber);
    const activeApps = useAudioStore(s => s.activeApps);
    const hardwareOutputs = useAudioStore(s => s.hardwareOutputs);

    return (
        <div className="flex flex-col gap-6 w-full max-w-5xl mx-auto pb-8 select-none">
            {/* Page Title */}
            <div>
                <h1 className="text-xl font-bold tracking-tight text-white font-sans">About</h1>
            </div>

            <div className="grid grid-cols-1 lg:grid-cols-2 gap-5">
                {/* Engine Status Card */}
                <div className="flex flex-col gap-5 rounded-3xl border-2 border-white/10 bg-[#202026] p-6">
                    <div className="flex items-center justify-between">
                        <div className="flex items-center gap-2.5">
                            <Server className="h-4 w-4 text-[#ff5511]" />
                            <h2 className="text-sm font-bold text-white tracking-tight">Engine Status</h2>
                        </div>

                        <div className="flex items-center gap-2 rounded-full border-2 border-white/10 bg-[#16161b] px-3 py-1">
                            <span className="relative flex h-2 w-2">
                                {status === "connected" ? (
                                    <>
                                        <span className="animate-ping absolute inline-flex h-full w-full rounded-full bg-emerald-400 opacity-75" />
                                        <span className="relative inline-flex rounded-full h-2 w-2 bg-emerald-500" />
                                    </>
                                ) : status === "connecting" ? (
                                    <span className="relative inline-flex rounded-full h-2 w-2 bg-amber-400 animate-pulse" />
                                ) : (
                                    <span className="relative inline-flex rounded-full h-2 w-2 bg-rose-500" />
                                )}
                            </span>
                            <span className="text-xs font-mono font-semibold text-white">
                                {status === "connected"
                                    ? "RUNNING"
                                    : status === "connecting"
                                      ? "CONNECTING"
                                      : "OFFLINE"}
                            </span>
                        </div>
                    </div>

                    <div className="grid grid-cols-1 sm:grid-cols-2 gap-3 text-xs font-mono">
                        <div className="rounded-2xl border-2 border-white/10 bg-[#16161b] p-3 flex flex-col gap-1">
                            <span className="text-zinc-400 text-[10px]">BACKEND</span>
                            <span className="font-semibold text-white">PipeWire 1.0+</span>
                        </div>

                        <div className="rounded-2xl border-2 border-white/10 bg-[#16161b] p-3 flex flex-col gap-1">
                            <span className="text-zinc-400 text-[10px]">DSP RATE</span>
                            <span className="font-semibold text-white">48 kHz · 32-bit float</span>
                        </div>

                        <div className="rounded-2xl border-2 border-white/10 bg-[#16161b] p-3 flex flex-col gap-1">
                            <span className="text-zinc-400 text-[10px]">IPC LATENCY</span>
                            <span className="font-semibold text-[#ff5511]">
                                {status === "connected" ? `${latency} ms` : "---"}
                            </span>
                        </div>

                        <div className="rounded-2xl border-2 border-white/10 bg-[#16161b] p-3 flex flex-col gap-1">
                            <span className="text-zinc-400 text-[10px]">STREAM STATE</span>
                            <span className="font-semibold text-emerald-400">
                                {isAudioActive ? "ACTIVE" : "STANDBY"}
                            </span>
                        </div>
                    </div>

                    <div className="rounded-2xl border-2 border-white/10 bg-[#16161b] p-3.5 flex flex-col gap-2">
                        <span className="text-zinc-400 text-[10px] font-mono">TELEMETRY BUS</span>
                        <div className="flex items-center justify-between text-xs font-mono text-zinc-300">
                            <div className="flex items-center gap-1.5">
                                <Activity className="h-3.5 w-3.5 text-[#ff5511]" />
                                <span>Peak / RMS</span>
                            </div>
                            <span className="font-semibold text-white">
                                {status === "connected" ? `${peakDb.toFixed(1)} dB / ${rmsDb.toFixed(1)} dB` : "-∞ dB"}
                            </span>
                        </div>
                        <div className="flex items-center justify-between text-xs font-mono text-zinc-300">
                            <div className="flex items-center gap-1.5">
                                <Radio className="h-3.5 w-3.5 text-zinc-400" />
                                <span>Sequence #</span>
                            </div>
                            <span className="text-zinc-400">{seq}</span>
                        </div>
                    </div>
                </div>

                {/* Application Details Card */}
                <div className="flex flex-col gap-5 rounded-3xl border-2 border-white/10 bg-[#202026] p-6">
                    <div className="flex items-center justify-between">
                        <div className="flex items-center gap-2.5">
                            <Info className="h-4 w-4 text-[#ff5511]" />
                            <h2 className="text-sm font-bold text-white tracking-tight">Application</h2>
                        </div>

                        <span className="rounded-full border-2 border-[#ff5511]/30 bg-[#ff5511]/10 px-3 py-1 text-xs font-mono font-bold text-[#ff5511]">
                            v0.1.0
                        </span>
                    </div>

                    <div className="flex items-center gap-3.5 rounded-2xl border-2 border-white/10 bg-[#16161b] p-4">
                        <div className="flex h-11 w-11 shrink-0 items-center justify-center rounded-xl bg-[#ff5511]/15 border-2 border-[#ff5511]/30 text-[#ff5511]">
                            <AudioWaveform className="h-6 w-6" />
                        </div>
                        <div className="flex flex-col">
                            <span className="text-base font-bold text-white tracking-tight">Routune Studio</span>
                            <span className="text-xs font-mono text-zinc-400">
                                PipeWire Audio Routing & Mixing Studio
                            </span>
                        </div>
                    </div>

                    <div className="grid grid-cols-1 sm:grid-cols-2 gap-3 text-xs font-mono">
                        <div className="rounded-2xl border-2 border-white/10 bg-[#16161b] p-3 flex flex-col gap-1">
                            <span className="text-zinc-400 text-[10px]">ACTIVE APPS</span>
                            <span className="font-semibold text-white">{activeApps.length}</span>
                        </div>

                        <div className="rounded-2xl border-2 border-white/10 bg-[#16161b] p-3 flex flex-col gap-1">
                            <span className="text-zinc-400 text-[10px]">OUTPUT SINKS</span>
                            <span className="font-semibold text-white">{hardwareOutputs.length}</span>
                        </div>

                        <div className="rounded-2xl border-2 border-white/10 bg-[#16161b] p-3 flex flex-col gap-1">
                            <span className="text-zinc-400 text-[10px]">GUI FRAMEWORK</span>
                            <span className="font-semibold text-white">Tauri v2 · React 18</span>
                        </div>

                        <div className="rounded-2xl border-2 border-white/10 bg-[#16161b] p-3 flex flex-col gap-1">
                            <span className="text-zinc-400 text-[10px]">LICENSE</span>
                            <span className="font-semibold text-white">GPL-3.0-only</span>
                        </div>
                    </div>
                </div>
            </div>
        </div>
    );
}
