import React, { useState, useEffect, useRef } from "react";
import {
  Monitor,
  Cpu,
  Keyboard,
  Database,
  AlertTriangle,
  Check,
  Download,
  Upload,
  RotateCcw,
  Volume2,
  Mic,
  Sparkles,
  Info,
  RefreshCw,
} from "lucide-react";
import {
  useSettingsStore,
  type GlobalShortcuts,
} from "../store/useSettingsStore";
import { useAudioStore } from "../store/useAudioStore";
import { routuneClient } from "../services/grpcClient";
import {
  enable as enableAutostart,
  disable as disableAutostart,
  isEnabled as isAutostartEnabled,
} from "@tauri-apps/plugin-autostart";

// ─── Toggle Switch ───────────────────────────────────────────────────────────
// Matches the exact switch used in DspTuningView (AI Noise, Spatial Audio, etc.)
interface ToggleSwitchProps {
  checked: boolean;
  onChange: (val: boolean) => void;
  disabled?: boolean;
  label?: string;
}

function ToggleSwitch({ checked, onChange, disabled, label }: ToggleSwitchProps) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={`relative inline-flex h-6 w-11 shrink-0 p-0.5 items-center rounded-full transition-colors duration-200 cursor-pointer border-0 ${
        checked
          ? "bg-[#ff5511] shadow-[0_0_12px_rgba(255,85,17,0.4)]"
          : "bg-[#282832] ring-1 ring-white/10 hover:ring-white/20"
      } ${disabled ? "opacity-40 cursor-not-allowed" : ""}`}
    >
      <span
        className={`inline-block h-5 w-5 transform rounded-full bg-white shadow-md shadow-black/50 transition-transform duration-200 ease-out pointer-events-none ${
          checked ? "translate-x-5" : "translate-x-0"
        }`}
      />
    </button>
  );
}

// ─── Section Card ─────────────────────────────────────────────────────────────
interface SectionCardProps {
  icon: React.ReactNode;
  title: string;
  subtitle: string;
  children: React.ReactNode;
  danger?: boolean;
}

function SectionCard({ icon, title, subtitle, children, danger }: SectionCardProps) {
  return (
    <div
      className={`flex flex-col rounded-2xl sm:rounded-3xl border-2 p-4 sm:p-6 gap-4 sm:gap-5 ${
        danger
          ? "border-rose-500/30 bg-[#1f1618]"
          : "border-white/10 bg-[#202026]"
      }`}
    >
      {/* Card Header – matches DspTuningView card header pattern */}
      <div className="flex items-center gap-3">
        <div
          className={`flex h-9 w-9 sm:h-10 sm:w-10 items-center justify-center rounded-xl border shrink-0 ${
            danger
              ? "bg-rose-500/15 border-rose-500/30 text-rose-400"
              : "bg-[#ff5511]/15 border-[#ff5511]/30 text-[#ff5511]"
          }`}
        >
          {icon}
        </div>
        <div className="min-w-0">
          <h2 className={`text-sm font-bold tracking-tight truncate ${danger ? "text-rose-200" : "text-white"}`}>
            {title}
          </h2>
          <span className="text-[11px] font-mono text-zinc-400 truncate block">{subtitle}</span>
        </div>
      </div>

      {/* Divider */}
      <div className={`h-px w-full ${danger ? "bg-rose-500/15" : "bg-white/5"}`} />

      {children}
    </div>
  );
}

// ─── Row: Toggle Setting ──────────────────────────────────────────────────────
interface ToggleRowProps {
  label: string;
  description: string;
  checked: boolean;
  onChange: (val: boolean) => void;
  note?: string;
  disabled?: boolean;
  divider?: boolean;
}

function ToggleRow({ label, description, checked, onChange, note, disabled, divider = true }: ToggleRowProps) {
  return (
    <>
      <div className="flex items-center justify-between gap-3 sm:gap-4">
        <div className="flex flex-col min-w-0 pr-1 sm:pr-2">
          <span className="text-sm font-medium text-white">{label}</span>
          <span className="text-xs text-zinc-400 mt-0.5 leading-relaxed">{description}</span>
          {note && <span className="text-[11px] font-mono text-zinc-500 mt-0.5">{note}</span>}
        </div>
        <div className="shrink-0">
          <ToggleSwitch checked={checked} onChange={onChange} disabled={disabled} label={label} />
        </div>
      </div>
      {divider && <div className="h-px w-full bg-white/5" />}
    </>
  );
}

// ─── Main Component ───────────────────────────────────────────────────────────
export function Settings() {
  const {
    runOnStartup,
    startMinimized,
    closeToTray,
    latencyMode,
    shortcuts,
    setRunOnStartup,
    setStartMinimized,
    setCloseToTray,
    setLatencyMode,
    setShortcut,
    resetSettings,
  } = useSettingsStore();

  const [autostartAvailable, setAutostartAvailable] = useState(true);
  const [recordingKey, setRecordingKey] = useState<keyof GlobalShortcuts | null>(null);
  const [feedbackMsg, setFeedbackMsg] = useState<string | null>(null);
  const [isRestarting, setIsRestarting] = useState(false);
  const [confirmReset, setConfirmReset] = useState(false);
  const fileInputRef = useRef<HTMLInputElement>(null);

  // Sync autostart state with Tauri plugin on mount
  useEffect(() => {
    let alive = true;
    isAutostartEnabled()
      .then((enabled) => { if (alive) setRunOnStartup(enabled); })
      .catch(() => { if (alive) setAutostartAvailable(false); });
    return () => { alive = false; };
  }, [setRunOnStartup]);

  const handleToggleAutostart = async (enabled: boolean) => {
    setRunOnStartup(enabled);
    try {
      enabled ? await enableAutostart() : await disableAutostart();
    } catch (err) {
      console.warn("autostart:", err);
    }
  };

  // Keyboard shortcut capture
  useEffect(() => {
    if (!recordingKey) return;
    const onKeyDown = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopPropagation();
      if (["Control", "Shift", "Alt", "Meta"].includes(e.key)) return;
      if (e.key === "Escape") { setRecordingKey(null); return; }

      const parts: string[] = [];
      if (e.ctrlKey) parts.push("Ctrl");
      if (e.metaKey) parts.push("Cmd");
      if (e.altKey) parts.push("Alt");
      if (e.shiftKey) parts.push("Shift");
      let k = e.code.startsWith("Key") ? e.code.slice(3)
        : e.code.startsWith("Digit") ? e.code.slice(5)
        : e.key.toUpperCase();
      parts.push(k);
      const combo = parts.join("+");
      setShortcut(recordingKey, combo);
      setRecordingKey(null);
      flash(`Shortcut for "${actionLabel(recordingKey)}" set to ${combo}`);
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [recordingKey, setShortcut]);

  const flash = (msg: string) => {
    setFeedbackMsg(msg);
    setTimeout(() => setFeedbackMsg((cur) => cur === msg ? null : cur), 3000);
  };

  const actionLabel = (k: keyof GlobalShortcuts) =>
    k === "muteMic" ? "Mute Microphone"
    : k === "muteMaster" ? "Mute Master"
    : "Toggle AI Noise";

  // Export config
  const handleExport = () => {
    try {
      const audio = useAudioStore.getState();
      const cfg = useSettingsStore.getState();
      const payload = {
        app: "Routune", schemaVersion: 1, exportedAt: new Date().toISOString(),
        settings: {
          runOnStartup: cfg.runOnStartup, startMinimized: cfg.startMinimized,
          closeToTray: cfg.closeToTray,
          latencyMode: cfg.latencyMode, shortcuts: cfg.shortcuts,
        },
        audioProfiles: {
          channelVolumes: audio.channelVolumes, channelMuted: audio.channelMuted,
          eqBands: audio.eqBands, channelNoiseEnabled: audio.channelNoiseEnabled,
        },
      };
      const url = "data:text/json;charset=utf-8," + encodeURIComponent(JSON.stringify(payload, null, 2));
      const a = document.createElement("a");
      a.href = url;
      a.download = `routune-config-${new Date().toISOString().slice(0, 10)}.json`;
      document.body.appendChild(a); a.click(); a.remove();
      flash("Configuration exported.");
    } catch { flash("Export failed."); }
  };

  // Import config
  const handleImport = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file) return;
    try {
      const json = JSON.parse(await file.text());
      const s = json.settings ?? {};
      if (typeof s.runOnStartup === "boolean") setRunOnStartup(s.runOnStartup);
      if (typeof s.startMinimized === "boolean") setStartMinimized(s.startMinimized);
      if (typeof s.closeToTray === "boolean") setCloseToTray(s.closeToTray);
      if (s.latencyMode) setLatencyMode(s.latencyMode);
      if (s.shortcuts) {
        const sc = s.shortcuts;
        if (sc.muteMic) setShortcut("muteMic", sc.muteMic);
        if (sc.muteMaster) setShortcut("muteMaster", sc.muteMaster);
        if (sc.toggleAiNoise) setShortcut("toggleAiNoise", sc.toggleAiNoise);
      }
      flash("Configuration imported.");
    } catch { flash("Invalid file format."); }
    finally { if (fileInputRef.current) fileInputRef.current.value = ""; }
  };

  // Restart engine
  const handleRestart = async () => {
    setIsRestarting(true);
    try {
      await routuneClient.ping({ clientId: "settings-restart" });
      await Promise.all([
        useAudioStore.getState().fetchHardwareOutputs(),
        useAudioStore.getState().fetchHardwareInputs(),
        useAudioStore.getState().fetchDaemonConfig(),
      ]);
      flash("Audio Engine restarted and devices synced.");
    } catch { flash("Restart signal sent. Awaiting daemon reconnect."); }
    finally { setTimeout(() => setIsRestarting(false), 1200); }
  };

  // Reset
  const handleReset = () => {
    if (!confirmReset) {
      setConfirmReset(true);
      setTimeout(() => setConfirmReset(false), 4000);
      return;
    }
    resetSettings();
    setConfirmReset(false);
    flash("All settings reset to factory defaults.");
  };

  const LATENCY_OPTIONS = [
    {
      id: "ultra_low" as const,
      label: "Ultra Low",
      spec: "64 samples · ~1.3ms",
      desc: "Minimum latency for competitive play. Highest CPU usage.",
    },
    {
      id: "balanced" as const,
      label: "Balanced",
      spec: "256 samples · ~5.3ms",
      desc: "Recommended. Optimal CPU efficiency with imperceptible delay.",
    },
    {
      id: "stable" as const,
      label: "Stable",
      spec: "512 samples · ~10.6ms",
      desc: "Maximum buffer headroom. Prevents xruns under heavy system load.",
    },
  ] as const;

  const SHORTCUT_ROWS: Array<{
    key: keyof GlobalShortcuts;
    label: string;
    sub: string;
    icon: React.ReactNode;
  }> = [
    {
      key: "muteMic",
      label: "Mute Microphone",
      sub: "Toggle the Routune Mic virtual source on/off",
      icon: <Mic className="h-4 w-4" />,
    },
    {
      key: "muteMaster",
      label: "Mute Master Audio",
      sub: "Mute all output channels sent to physical sinks",
      icon: <Volume2 className="h-4 w-4" />,
    },
    {
      key: "toggleAiNoise",
      label: "Toggle AI Noise Suppression",
      sub: "Enable or disable RNNoise neural filter on the fly",
      icon: <Sparkles className="h-4 w-4 text-[#ff5511]" />,
    },
  ];

  return (
    <div className="flex flex-col gap-4 sm:gap-6 w-full max-w-7xl mx-auto pb-8 select-none">
      {/* Page Title Row – identical style to Tuning / Routing */}
      <div className="flex flex-wrap items-center justify-between gap-3">
        <h1 className="text-xl font-bold tracking-tight text-white font-sans">Settings</h1>

        {/* Inline feedback badge */}
        {feedbackMsg && (
          <div className="flex items-center gap-2 rounded-full border border-[#ff5511]/30 bg-[#ff5511]/10 px-3.5 py-1.5 text-xs font-medium text-[#ff5511] font-mono">
            <Check className="h-3.5 w-3.5 shrink-0" />
            <span>{feedbackMsg}</span>
          </div>
        )}
      </div>

      {/* ── 1. System Behavior ─────────────────────────────────────────────── */}
      <SectionCard
        icon={<Monitor className="h-5 w-5" />}
        title="System Behavior"
        subtitle="Launch preferences · Window lifecycle"
      >
        <div className="flex flex-col gap-3.5 sm:gap-4">
          <ToggleRow
            label="Launch Routune on system startup"
            description="Automatically start the audio routing daemon when logging into your desktop session."
            note={!autostartAvailable ? "(systemd user unit recommended on Linux)" : undefined}
            checked={runOnStartup}
            onChange={handleToggleAutostart}
          />
          <ToggleRow
            label="Start minimized to system tray"
            description="Launch silently in the background without opening the main window or stealing focus."
            checked={startMinimized}
            onChange={setStartMinimized}
          />
          <ToggleRow
            label="Keep daemon running when window is closed"
            description="Closing the window hides the GUI to background while preserving uninterrupted audio routing."
            checked={closeToTray}
            onChange={setCloseToTray}
            divider={false}
          />
        </div>
      </SectionCard>

      {/* ── 2. Audio Engine ────────────────────────────────────────────────── */}
      <SectionCard
        icon={<Cpu className="h-5 w-5" />}
        title="Audio Engine"
        subtitle="DSP frame buffer latency targets"
      >
        <div className="flex flex-col gap-2">
          <span className="text-[10px] font-mono uppercase tracking-widest text-zinc-500">
            Latency Target
          </span>

          {/* Responsive grid: stacks cleanly on minimum window, spreads to 3-cols on wider desktop */}
          <div className="grid grid-cols-1 lg:grid-cols-3 gap-2.5">
            {LATENCY_OPTIONS.map((opt) => {
              const active = latencyMode === opt.id;
              return (
                <button
                  key={opt.id}
                  type="button"
                  onClick={() => setLatencyMode(opt.id)}
                  className={`flex flex-col items-start rounded-xl sm:rounded-2xl border-2 p-3.5 sm:p-4 text-left transition-all cursor-pointer ${
                    active
                      ? "border-[#ff5511] bg-[#ff5511]/10 shadow-[0_0_16px_rgba(255,85,17,0.15)]"
                      : "border-white/10 bg-[#16161b] hover:border-white/20 hover:bg-[#282830]"
                  }`}
                >
                  <div className="flex flex-wrap items-center justify-between gap-1.5 w-full mb-1.5">
                    <span className={`text-sm font-semibold ${active ? "text-white" : "text-zinc-200"}`}>
                      {opt.label}
                    </span>
                    <span
                      className={`text-[10px] font-mono px-2 py-0.5 rounded-full border shrink-0 ${
                        active
                          ? "border-[#ff5511]/40 bg-[#ff5511]/15 text-[#ff5511]"
                          : "border-white/10 bg-white/5 text-zinc-400"
                      }`}
                    >
                      {opt.spec}
                    </span>
                  </div>
                  <span className="text-xs text-zinc-400 leading-relaxed">{opt.desc}</span>
                </button>
              );
            })}
          </div>
        </div>
      </SectionCard>

      {/* ── 3. Global Shortcuts ────────────────────────────────────────────── */}
      <SectionCard
        icon={<Keyboard className="h-5 w-5" />}
        title="Global Shortcuts"
        subtitle="System-wide hotkeys active across all running applications"
      >
        <div className="flex flex-col gap-3.5 sm:gap-4">
          {SHORTCUT_ROWS.map(({ key, label, sub, icon }, idx) => {
            const isRecording = recordingKey === key;
            const val = shortcuts[key];
            return (
              <React.Fragment key={key}>
                <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-2.5 sm:gap-4">
                  {/* Icon + labels */}
                  <div className="flex items-center gap-3 min-w-0">
                    <div className="flex h-9 w-9 sm:h-10 sm:w-10 items-center justify-center rounded-xl border border-white/10 bg-[#16161b] text-zinc-300 shrink-0">
                      {icon}
                    </div>
                    <div className="flex flex-col min-w-0">
                      <span className="text-sm font-medium text-white">{label}</span>
                      <span className="text-[11px] font-mono text-zinc-400 truncate">{sub}</span>
                    </div>
                  </div>

                  {/* Capture button + clear */}
                  <div className="flex items-center gap-1.5 self-end sm:self-auto shrink-0">
                    <button
                      type="button"
                      onClick={() => setRecordingKey(isRecording ? null : key)}
                      className={`min-w-28 sm:min-w-32 rounded-xl border-2 px-3 py-1.5 text-xs font-mono font-medium transition-all cursor-pointer ${
                        isRecording
                          ? "border-[#ff5511] bg-[#ff5511]/15 text-[#ff5511] shadow-[0_0_12px_rgba(255,85,17,0.3)] animate-pulse"
                          : "border-white/10 bg-[#16161b] text-zinc-200 hover:border-white/20 hover:bg-[#282830]"
                      }`}
                    >
                      {isRecording ? "Press keys…" : val || "— unbound —"}
                    </button>

                    {val && !isRecording && (
                      <button
                        type="button"
                        title="Clear shortcut"
                        onClick={() => setShortcut(key, "")}
                        className="flex h-8 w-8 items-center justify-center rounded-xl border-2 border-white/10 bg-[#16161b] hover:bg-[#282830] hover:border-white/20 text-zinc-400 hover:text-zinc-200 transition-all cursor-pointer"
                      >
                        <RotateCcw className="h-3.5 w-3.5" />
                      </button>
                    )}
                  </div>
                </div>

                {idx < SHORTCUT_ROWS.length - 1 && (
                  <div className="h-px w-full bg-white/5" />
                )}
              </React.Fragment>
            );
          })}
        </div>

        {/* Info callout */}
        <div className="flex items-center gap-2.5 rounded-xl sm:rounded-2xl border border-white/5 bg-[#16161b] px-3.5 sm:px-4 py-2.5 text-xs font-mono text-zinc-400">
          <Info className="h-3.5 w-3.5 shrink-0 text-[#ff5511]" />
          <span>Click a binding, then press any key combination. Press Esc to cancel.</span>
        </div>
      </SectionCard>

      {/* ── 4. Data & Profiles ─────────────────────────────────────────────── */}
      <SectionCard
        icon={<Database className="h-5 w-5" />}
        title="Data & Profiles"
        subtitle="Export and import EQ presets, volumes, and preferences"
      >
        <input
          type="file"
          ref={fileInputRef}
          accept=".json"
          onChange={handleImport}
          className="hidden"
        />
        <div className="flex flex-col sm:flex-row gap-2.5 sm:gap-3">
          <button
            type="button"
            onClick={handleExport}
            className="flex-1 flex items-center justify-center gap-2 rounded-xl sm:rounded-2xl border-2 border-white/10 bg-[#16161b] hover:bg-[#282830] hover:border-white/20 px-4 py-2.5 sm:py-3 text-xs font-semibold text-zinc-200 hover:text-white transition-all cursor-pointer"
          >
            <Download className="h-4 w-4 text-[#ff5511] shrink-0" />
            <span className="truncate">Export Configuration</span>
          </button>
          <button
            type="button"
            onClick={() => fileInputRef.current?.click()}
            className="flex-1 flex items-center justify-center gap-2 rounded-xl sm:rounded-2xl border-2 border-white/10 bg-[#16161b] hover:bg-[#282830] hover:border-white/20 px-4 py-2.5 sm:py-3 text-xs font-semibold text-zinc-200 hover:text-white transition-all cursor-pointer"
          >
            <Upload className="h-4 w-4 text-[#ff5511] shrink-0" />
            <span className="truncate">Import Configuration</span>
          </button>
        </div>
      </SectionCard>

      {/* ── 5. Danger Zone ─────────────────────────────────────────────────── */}
      <SectionCard
        icon={<AlertTriangle className="h-5 w-5" />}
        title="Danger Zone"
        subtitle="Audio engine restart · Factory reset"
        danger
      >
        {/* Restart row */}
        <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-3 sm:gap-4">
          <div className="flex flex-col min-w-0">
            <span className="text-sm font-medium text-white">Restart Audio Engine</span>
            <span className="text-xs text-zinc-400 mt-0.5 leading-relaxed">
              Re-initializes hardware streams, rebinds PipeWire nodes, and restores the DSP graph.
            </span>
          </div>
          <button
            type="button"
            disabled={isRestarting}
            onClick={handleRestart}
            className="self-start sm:self-auto shrink-0 flex items-center justify-center gap-2 rounded-xl border-2 border-rose-500/30 bg-rose-500/10 hover:bg-rose-500/20 hover:border-rose-500/50 px-4 py-2 text-xs font-semibold text-rose-300 hover:text-rose-100 transition-all cursor-pointer disabled:opacity-40 disabled:cursor-not-allowed"
          >
            <RefreshCw className={`h-3.5 w-3.5 ${isRestarting ? "animate-spin" : ""}`} />
            <span className="whitespace-nowrap">{isRestarting ? "Restarting…" : "Restart Engine"}</span>
          </button>
        </div>

        <div className="h-px w-full bg-rose-500/15" />

        {/* Reset row */}
        <div className="flex flex-col sm:flex-row sm:items-center justify-between gap-3 sm:gap-4">
          <div className="flex flex-col min-w-0">
            <span className="text-sm font-medium text-white">Reset All Settings</span>
            <span className="text-xs text-zinc-400 mt-0.5 leading-relaxed">
              Revert all buffer targets, shortcuts, and behavioral flags to factory defaults.
            </span>
          </div>
          <button
            type="button"
            onClick={handleReset}
            className={`self-start sm:self-auto shrink-0 flex items-center justify-center gap-2 rounded-xl border-2 px-4 py-2 text-xs font-semibold transition-all cursor-pointer whitespace-nowrap ${
              confirmReset
                ? "border-rose-500 bg-rose-600 text-white animate-pulse"
                : "border-rose-500/30 bg-rose-500/10 hover:bg-rose-500/20 hover:border-rose-500/50 text-rose-300 hover:text-rose-100"
            }`}
          >
            <RotateCcw className="h-3.5 w-3.5 shrink-0" />
            <span>{confirmReset ? "Confirm Reset?" : "Reset Settings"}</span>
          </button>
        </div>
      </SectionCard>
    </div>
  );
}

export default Settings;
