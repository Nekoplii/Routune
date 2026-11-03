import { create } from "zustand";
import type { AppStream, ChannelVolumeMetric, OutputDeviceInfo } from "../proto/routune";
import { AudioChannel } from "../proto/routune";
import { routuneClient } from "../services/grpcClient";

export type ConnectionStatus = "disconnected" | "connecting" | "connected" | "error";
export type NavigationView = "mixer" | "routing" | "tuning" | "about";

export interface NoiseGateState {
  enabled: boolean;
  thresholdDb: number;
  attackMs: number;
  releaseMs: number;
}

export interface CompressorState {
  enabled: boolean;
  thresholdDb: number;
  ratio: number;
  attackMs: number;
  releaseMs: number;
  makeupGainDb: number;
}

interface AudioStoreState {
  connectionStatus: ConnectionStatus;
  isAudioActive: boolean;
  latencyMs: number;
  lastError: string | null;
  activeView: NavigationView;

  // Real-time telemetry (60 FPS stream)
  peakDb: number;
  rmsDb: number;
  sequenceNumber: string;
  channelMetrics: ChannelVolumeMetric[];
  channelPeakHolds: Record<number, number>;

  // Mixer Fader & Mute State
  channelVolumes: Record<number, number>; // 0 to 100
  channelFaderDb: Record<number, number>; // -48 to +6 dB
  channelMuted: Record<number, boolean>;

  // Hardware routing (Sonar model)
  hardwareOutputs: OutputDeviceInfo[];
  hardwareInputs: OutputDeviceInfo[];
  channelRoutings: Record<number, string>;
  isRoutingUpdating: Record<number, boolean>;

  // Application routing (Sonar app assignment)
  activeApps: AppStream[];
  isRoutingApp: Record<number, boolean>;

  // 10-Band Parametric Equalizer State
  selectedEqChannel: AudioChannel;
  eqBands: Record<number, number[]>;

  // AI Noise Suppression (RNNoise)
  channelNoiseEnabled: Record<number, boolean>;
  isTogglingAiNoise: Record<number, boolean>;

  // Mic Dynamics: Noise Gate & Compressor
  noiseGate: NoiseGateState;
  compressor: CompressorState;

  // Mic Test / Live Monitor
  micTestEnabled: boolean;
  isTogglingMicTest: boolean;

  // Actions
  setConnectionStatus: (status: ConnectionStatus, error?: string | null) => void;
  setAudioActive: (active: boolean) => void;
  setLatencyMs: (ms: number) => void;
  updateTelemetry: (
    peakDb: number,
    rmsDb: number,
    seq: string,
    channelMetrics?: ChannelVolumeMetric[]
  ) => void;
  resetTelemetry: () => void;
  fetchHardwareOutputs: () => Promise<void>;
  fetchHardwareInputs: () => Promise<void>;
  fetchActiveApps: () => Promise<void>;
  fetchDaemonConfig: () => Promise<void>;
  fetchMicTest: () => Promise<void>;
  toggleMicTest: () => Promise<boolean>;
  routeApplication: (appId: number, targetChannel: AudioChannel) => Promise<boolean>;
  setChannelRouting: (channel: AudioChannel, deviceName: string) => Promise<boolean>;
  setSelectedEqChannel: (channel: AudioChannel) => void;
  setEqBandGain: (channel: AudioChannel, bandIndex: number, gainDb: number) => void;
  resetChannelEq: (channel: AudioChannel) => void;
  toggleAiNoise: (channel: AudioChannel, enabled: boolean) => Promise<boolean>;
  setNoiseGate: (params: Partial<NoiseGateState>) => Promise<boolean>;
  setCompressor: (params: Partial<CompressorState>) => Promise<boolean>;
  setActiveView: (view: NavigationView) => void;
  setChannelVolume: (channel: AudioChannel, volume: number) => void;
  setChannelFaderDb: (channel: AudioChannel, db: number) => void;
  resetAllFaders: () => void;
  toggleChannelMute: (channel: AudioChannel) => void;
}

const DEFAULT_EQ_BANDS = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
const GAME_EQ_BANDS = [-4, 10, 1, 0, 0, 0, 0, 0, 0, 0];

const DEFAULT_HARDWARE_OUTPUTS: OutputDeviceInfo[] = [
  {
    id: "alsa_output.usb-headset",
    name: "alsa_output.usb-headset",
    displayName: "USB Headset",
    channels: 2,
    sampleRate: 48000,
  },
  {
    id: "alsa_output.pci-builtin",
    name: "alsa_output.pci-builtin",
    displayName: "Speakers (Built-in Audio)",
    channels: 2,
    sampleRate: 48000,
  },
];

const DEFAULT_HARDWARE_INPUTS: OutputDeviceInfo[] = [
  {
    id: "sysdefault:CARD=Generic_1",
    name: "sysdefault:CARD=Generic_1",
    displayName: "Ryzen HD Audio",
    channels: 2,
    sampleRate: 48000,
  },
];

const INITIAL_APPS: AppStream[] = [
  {
    appId: 131,
    name: "Counter-Strike 2",
    currentSink: AudioChannel.GAME,
    currentSinkName: "Routune Game",
    iconName: "cs2",
  },
  {
    appId: 112,
    name: "Discord",
    currentSink: AudioChannel.CHAT,
    currentSinkName: "Routune Chat",
    iconName: "discord",
  },
  {
    appId: 94,
    name: "Spotify",
    currentSink: AudioChannel.MEDIA,
    currentSinkName: "Routune Media",
    iconName: "spotify",
  },
  {
    appId: 77,
    name: "Firefox",
    currentSink: AudioChannel.MASTER,
    currentSinkName: "Default Output",
    iconName: "firefox",
  },
  {
    appId: 42,
    name: "System Sounds",
    currentSink: AudioChannel.MASTER,
    currentSinkName: "Default Output",
    iconName: "system",
  },
];

const volumeDebounceTimers: Record<number, ReturnType<typeof setTimeout>> = {};
let noiseGateDebounceTimer: ReturnType<typeof setTimeout> | null = null;
let compressorDebounceTimer: ReturnType<typeof setTimeout> | null = null;

export const DEFAULT_NOISE_GATE: NoiseGateState = {
  enabled: false,
  thresholdDb: -40.0,
  attackMs: 5.0,
  releaseMs: 100.0,
};

export const DEFAULT_COMPRESSOR: CompressorState = {
  enabled: false,
  thresholdDb: -18.0,
  ratio: 4.0,
  attackMs: 10.0,
  releaseMs: 100.0,
  makeupGainDb: 0.0,
};

interface CachedConfig {
  channelFaderDb: Record<number, number>;
  channelMuted: Record<number, boolean>;
  channelRoutings: Record<number, string>;
  channelNoiseEnabled: Record<number, boolean>;
  eqBands: Record<number, number[]>;
  noiseGate?: NoiseGateState;
  compressor?: CompressorState;
}

function loadLocalConfig(): Partial<CachedConfig> {
  if (typeof localStorage === "undefined") return {};
  try {
    const raw = localStorage.getItem("routune_cached_config");
    return raw ? JSON.parse(raw) : {};
  } catch {
    return {};
  }
}

function saveLocalConfig(partial: Partial<CachedConfig>) {
  if (typeof localStorage === "undefined") return;
  try {
    const existing = loadLocalConfig();
    const updated = { ...existing, ...partial };
    localStorage.setItem("routune_cached_config", JSON.stringify(updated));
  } catch {}
}

const initialCached = loadLocalConfig();

function dispatchChannelVolume(channel: AudioChannel, volumeDb: number) {
  if (volumeDebounceTimers[channel]) {
    clearTimeout(volumeDebounceTimers[channel]);
  }
  volumeDebounceTimers[channel] = setTimeout(() => {
    delete volumeDebounceTimers[channel];
    routuneClient.setChannelVolume({ channel, volumeDb }).response.catch((err) => {
      console.warn(`Failed to set channel volume for ${channel}:`, err);
    });
  }, 25);
}

function dispatchChannelMute(channel: AudioChannel, muted: boolean) {
  routuneClient.setChannelMute({ channel, muted }).response.catch((err) => {
    console.warn(`Failed to set channel mute for ${channel}:`, err);
  });
}

export const useAudioStore = create<AudioStoreState>((set, get) => ({
  connectionStatus: "disconnected",
  isAudioActive: false,
  latencyMs: 5.3,
  lastError: null,
  activeView: "mixer",

  peakDb: -14.6,
  rmsDb: -22.4,
  sequenceNumber: "0",
  channelMetrics: [],
  channelPeakHolds: {
    [AudioChannel.MASTER]: -14.6,
    [AudioChannel.GAME]: -14.6,
    [AudioChannel.CHAT]: -18.2,
    [AudioChannel.MEDIA]: -12.4,
    [AudioChannel.AUX]: -24.0,
    [AudioChannel.MIC]: -18.0,
  },

  channelVolumes: {
    [AudioChannel.MASTER]: 85,
    [AudioChannel.GAME]: 85,
    [AudioChannel.CHAT]: 85,
    [AudioChannel.MEDIA]: 85,
    [AudioChannel.AUX]: 85,
    [AudioChannel.MIC]: 85,
  },
  channelFaderDb: {
    [AudioChannel.MASTER]: 0.0,
    [AudioChannel.GAME]: 0.0,
    [AudioChannel.CHAT]: 0.0,
    [AudioChannel.MEDIA]: 0.0,
    [AudioChannel.AUX]: 0.0,
    [AudioChannel.MIC]: 0.0,
    ...initialCached.channelFaderDb,
  },
  channelMuted: {
    [AudioChannel.MASTER]: false,
    [AudioChannel.GAME]: false,
    [AudioChannel.CHAT]: false,
    [AudioChannel.MEDIA]: false,
    [AudioChannel.AUX]: false,
    [AudioChannel.MIC]: false,
    ...initialCached.channelMuted,
  },

  hardwareOutputs: DEFAULT_HARDWARE_OUTPUTS,
  hardwareInputs: DEFAULT_HARDWARE_INPUTS,
  channelRoutings: {
    [AudioChannel.GAME]: "alsa_output.usb-headset",
    [AudioChannel.CHAT]: "alsa_output.usb-headset",
    [AudioChannel.MEDIA]: "alsa_output.pci-builtin",
    [AudioChannel.AUX]: "none",
    [AudioChannel.MIC]: "none",
    ...initialCached.channelRoutings,
  },
  isRoutingUpdating: {},

  activeApps: INITIAL_APPS,
  isRoutingApp: {},

  selectedEqChannel: AudioChannel.GAME,
  eqBands: {
    [AudioChannel.GAME]: [...GAME_EQ_BANDS],
    [AudioChannel.CHAT]: [...DEFAULT_EQ_BANDS],
    [AudioChannel.MEDIA]: [...DEFAULT_EQ_BANDS],
    [AudioChannel.AUX]: [...DEFAULT_EQ_BANDS],
    [AudioChannel.MIC]: [...DEFAULT_EQ_BANDS],
    ...initialCached.eqBands,
  },

  channelNoiseEnabled: {
    [AudioChannel.GAME]: false,
    [AudioChannel.CHAT]: false,
    [AudioChannel.MEDIA]: false,
    [AudioChannel.AUX]: false,
    [AudioChannel.MIC]: false,
    ...initialCached.channelNoiseEnabled,
  },
  isTogglingAiNoise: {},

  noiseGate: initialCached.noiseGate ?? DEFAULT_NOISE_GATE,
  compressor: initialCached.compressor ?? DEFAULT_COMPRESSOR,

  micTestEnabled: false,
  isTogglingMicTest: false,

  setConnectionStatus: (status, error = null) =>
    set({
      connectionStatus: status,
      lastError: error,
      ...(status !== "connected"
        ? { isAudioActive: false, peakDb: -100, rmsDb: -100, channelMetrics: [] }
        : {}),
    }),

  setAudioActive: (isAudioActive) => set({ isAudioActive }),

  setLatencyMs: (latencyMs) => set({ latencyMs }),

  updateTelemetry: (peakDb, rmsDb, sequenceNumber, channelMetrics = []) =>
    set((s) => {
      let updatedNoise = s.channelNoiseEnabled;
      if (channelMetrics.length > 0) {
        let changed = false;
        const nextNoise = { ...s.channelNoiseEnabled };
        for (const metric of channelMetrics) {
          if (!s.isTogglingAiNoise[metric.channel] && metric.channel in nextNoise) {
            if (nextNoise[metric.channel] !== metric.aiNoiseEnabled) {
              nextNoise[metric.channel] = metric.aiNoiseEnabled;
              changed = true;
            }
          }
        }
        if (changed) {
          updatedNoise = nextNoise;
        }
      }

      // Smooth peak hold decay
      const nextHolds = { ...s.channelPeakHolds };
      if (peakDb >= (nextHolds[AudioChannel.MASTER] ?? -100)) {
        nextHolds[AudioChannel.MASTER] = peakDb;
      } else {
        nextHolds[AudioChannel.MASTER] = Math.max(-100, (nextHolds[AudioChannel.MASTER] ?? -100) - 0.4);
      }

      for (const metric of channelMetrics) {
        const prev = nextHolds[metric.channel] ?? -100;
        if (metric.peakVolumeDb >= prev) {
          nextHolds[metric.channel] = metric.peakVolumeDb;
        } else {
          nextHolds[metric.channel] = Math.max(-100, prev - 0.4);
        }
      }

      return {
        peakDb,
        rmsDb,
        sequenceNumber,
        channelMetrics,
        channelNoiseEnabled: updatedNoise,
        channelPeakHolds: nextHolds,
      };
    }),

  resetTelemetry: () =>
    set({
      peakDb: -100,
      rmsDb: -100,
      sequenceNumber: "0",
      channelMetrics: [],
      channelPeakHolds: {
        [AudioChannel.MASTER]: -100,
        [AudioChannel.GAME]: -100,
        [AudioChannel.CHAT]: -100,
        [AudioChannel.MEDIA]: -100,
        [AudioChannel.AUX]: -100,
        [AudioChannel.MIC]: -100,
      },
    }),

  fetchHardwareOutputs: async () => {
    try {
      const call = routuneClient.getHardwareOutputs({});
      const res = await call.response;
      set({ hardwareOutputs: res.devices });
    } catch (err: unknown) {
      console.warn("Failed to fetch hardware outputs:", err);
    }
  },

  fetchHardwareInputs: async () => {
    try {
      const call = routuneClient.getHardwareInputs({});
      const res = await call.response;
      set({ hardwareInputs: res.devices });
    } catch (err: unknown) {
      console.warn("Failed to fetch hardware inputs:", err);
    }
  },

  fetchActiveApps: async () => {
    try {
      const call = routuneClient.getActiveApplications({});
      const res = await call.response;
      set((s) => {
        // Compare to prevent unnecessary React re-renders when state is unchanged
        if (s.activeApps.length === res.apps.length) {
          const isIdentical = s.activeApps.every((existing, idx) => {
            const incoming = res.apps[idx];
            return (
              incoming &&
              existing.appId === incoming.appId &&
              existing.name === incoming.name &&
              existing.currentSink === incoming.currentSink &&
              existing.currentSinkName === incoming.currentSinkName &&
              existing.iconName === incoming.iconName
            );
          });
          if (isIdentical) {
            return s;
          }
        }
        return { activeApps: res.apps };
      });
    } catch (err: unknown) {
      console.warn("Failed to fetch active applications:", err);
    }
  },

  fetchDaemonConfig: async () => {
    try {
      const call = routuneClient.getDaemonConfig({});
      const res = await call.response;
      if (!res.channels || res.channels.length === 0) return;

      const nextFaderDb: Record<number, number> = {};
      const nextMuted: Record<number, boolean> = {};
      const nextRoutings: Record<number, string> = {};
      const nextEqBands: Record<number, number[]> = {};
      const nextNoise: Record<number, boolean> = {};
      let nextNoiseGate: NoiseGateState | undefined;
      let nextCompressor: CompressorState | undefined;

      for (const chState of res.channels) {
        const ch = chState.channel as AudioChannel;
        nextFaderDb[ch] = chState.volumeDb;
        nextMuted[ch] = chState.muted;
        if (chState.routeDevice) {
          nextRoutings[ch] = chState.routeDevice;
        }
        if (chState.eqBands && chState.eqBands.length === 10) {
          nextEqBands[ch] = [...chState.eqBands];
        }
        nextNoise[ch] = chState.noiseCancellation;

        if (ch === AudioChannel.MIC) {
          if (chState.noiseGate) {
            nextNoiseGate = {
              enabled: chState.noiseGate.enabled,
              thresholdDb: chState.noiseGate.thresholdDb,
              attackMs: chState.noiseGate.attackMs,
              releaseMs: chState.noiseGate.releaseMs,
            };
          }
          if (chState.compressor) {
            nextCompressor = {
              enabled: chState.compressor.enabled,
              thresholdDb: chState.compressor.thresholdDb,
              ratio: chState.compressor.ratio,
              attackMs: chState.compressor.attackMs,
              releaseMs: chState.compressor.releaseMs,
              makeupGainDb: chState.compressor.makeupGainDb,
            };
          }
        }
      }

      set((s) => ({
        channelFaderDb: { ...s.channelFaderDb, ...nextFaderDb },
        channelMuted: { ...s.channelMuted, ...nextMuted },
        channelRoutings: { ...s.channelRoutings, ...nextRoutings },
        eqBands: { ...s.eqBands, ...nextEqBands },
        channelNoiseEnabled: { ...s.channelNoiseEnabled, ...nextNoise },
        ...(nextNoiseGate ? { noiseGate: nextNoiseGate } : {}),
        ...(nextCompressor ? { compressor: nextCompressor } : {}),
      }));

      saveLocalConfig({
        channelFaderDb: nextFaderDb,
        channelMuted: nextMuted,
        channelRoutings: nextRoutings,
        eqBands: nextEqBands,
        channelNoiseEnabled: nextNoise,
        ...(nextNoiseGate ? { noiseGate: nextNoiseGate } : {}),
        ...(nextCompressor ? { compressor: nextCompressor } : {}),
      });
    } catch (err: unknown) {
      console.warn("Failed to fetch daemon config:", err);
    }
  },

  fetchMicTest: async () => {
    try {
      const call = routuneClient.getMicTest({});
      const res = await call.response;
      set({ micTestEnabled: res.enabled });
    } catch (err: unknown) {
      console.warn("Failed to get mic test status:", err);
    }
  },

  toggleMicTest: async () => {
    const next = !get().micTestEnabled;
    set({ isTogglingMicTest: true, micTestEnabled: next });
    try {
      const call = routuneClient.setMicTest({ enabled: next });
      const res = await call.response;
      set({ micTestEnabled: res.enabled, isTogglingMicTest: false });
      return res.enabled;
    } catch (err: unknown) {
      console.warn("Failed to toggle mic test:", err);
      set({ micTestEnabled: !next, isTogglingMicTest: false });
      return !next;
    }
  },

  routeApplication: async (appId: number, targetChannel: AudioChannel) => {
    const prevApps = useAudioStore.getState().activeApps;
    const targetSinkName =
      targetChannel === AudioChannel.GAME
        ? "Routune Game"
        : targetChannel === AudioChannel.CHAT
        ? "Routune Chat"
        : targetChannel === AudioChannel.MEDIA
        ? "Routune Media"
        : targetChannel === AudioChannel.AUX
        ? "Routune Aux"
        : "Default Output";

    // Optimistically update activeApps state
    set((s) => ({
      isRoutingApp: { ...s.isRoutingApp, [appId]: true },
      activeApps: s.activeApps.map((app) =>
        app.appId === appId
          ? { ...app, currentSink: targetChannel, currentSinkName: targetSinkName }
          : app
      ),
    }));

    try {
      const call = routuneClient.routeApplication({ appId, targetChannel });
      await call.response;
      set((s) => ({
        isRoutingApp: { ...s.isRoutingApp, [appId]: false },
      }));
      return true;
    } catch (err: unknown) {
      console.warn(`Failed to route app ${appId} to ${targetChannel}:`, err);
      // Revert optimistic update on failure
      set((s) => ({
        activeApps: prevApps,
        isRoutingApp: { ...s.isRoutingApp, [appId]: false },
      }));
      return false;
    }
  },

  setChannelRouting: async (channel: AudioChannel, deviceName: string) => {
    set((s) => ({
      isRoutingUpdating: { ...s.isRoutingUpdating, [channel]: true },
    }));
    try {
      const call = routuneClient.setChannelRouting({
        channel,
        targetHardwareDeviceName: deviceName,
      });
      const res = await call.response;
      if (res.success) {
        set((s) => {
          const nextRoutings = { ...s.channelRoutings, [channel]: res.activeDeviceName };
          saveLocalConfig({ channelRoutings: nextRoutings });
          return {
            channelRoutings: nextRoutings,
            isRoutingUpdating: { ...s.isRoutingUpdating, [channel]: false },
          };
        });
        return true;
      } else {
        set((s) => ({
          lastError: res.message,
          isRoutingUpdating: { ...s.isRoutingUpdating, [channel]: false },
        }));
        return false;
      }
    } catch (err: unknown) {
      const msg = err instanceof Error ? err.message : "Failed to set routing";
      set((s) => ({
        lastError: msg,
        isRoutingUpdating: { ...s.isRoutingUpdating, [channel]: false },
      }));
      return false;
    }
  },

  setSelectedEqChannel: (channel) => set({ selectedEqChannel: channel }),

  setEqBandGain: (channel, bandIndex, gainDb) =>
    set((s) => {
      const currentBands = s.eqBands[channel] ? [...s.eqBands[channel]] : [...DEFAULT_EQ_BANDS];
      currentBands[bandIndex] = gainDb;
      const nextEqBands = {
        ...s.eqBands,
        [channel]: currentBands,
      };
      saveLocalConfig({ eqBands: nextEqBands });
      return {
        eqBands: nextEqBands,
      };
    }),

  resetChannelEq: (channel) =>
    set((s) => {
      const nextEqBands = {
        ...s.eqBands,
        [channel]: [...DEFAULT_EQ_BANDS],
      };
      saveLocalConfig({ eqBands: nextEqBands });
      return {
        eqBands: nextEqBands,
      };
    }),

  toggleAiNoise: async (channel: AudioChannel, enabled: boolean) => {
    const prevEnabled = useAudioStore.getState().channelNoiseEnabled[channel] ?? false;
    // Optimistic update
    set((s) => {
      const nextNoise = { ...s.channelNoiseEnabled, [channel]: enabled };
      saveLocalConfig({ channelNoiseEnabled: nextNoise });
      return {
        isTogglingAiNoise: { ...s.isTogglingAiNoise, [channel]: true },
        channelNoiseEnabled: nextNoise,
      };
    });

    try {
      const call = routuneClient.toggleAiNoise({ channel, enabled });
      await call.response;
      set((s) => ({
        isTogglingAiNoise: { ...s.isTogglingAiNoise, [channel]: false },
      }));
      return true;
    } catch (err: unknown) {
      console.warn(`Failed to toggle AI noise for channel ${channel}:`, err);
      // Revert optimistic update on failure
      set((s) => {
        const revertedNoise = { ...s.channelNoiseEnabled, [channel]: prevEnabled };
        saveLocalConfig({ channelNoiseEnabled: revertedNoise });
        return {
          channelNoiseEnabled: revertedNoise,
          isTogglingAiNoise: { ...s.isTogglingAiNoise, [channel]: false },
        };
      });
      return false;
    }
  },

  setNoiseGate: async (params: Partial<NoiseGateState>) => {
    const current = get().noiseGate;
    const next: NoiseGateState = { ...current, ...params };
    set({ noiseGate: next });
    saveLocalConfig({ noiseGate: next });

    if (noiseGateDebounceTimer) {
      clearTimeout(noiseGateDebounceTimer);
    }

    return new Promise<boolean>((resolve) => {
      noiseGateDebounceTimer = setTimeout(async () => {
        noiseGateDebounceTimer = null;
        try {
          const call = routuneClient.setNoiseGate({
            channel: AudioChannel.MIC,
            config: next,
          });
          await call.response;
          resolve(true);
        } catch (err) {
          console.warn("Failed to set noise gate:", err);
          resolve(false);
        }
      }, 50);
    });
  },

  setCompressor: async (params: Partial<CompressorState>) => {
    const current = get().compressor;
    const next: CompressorState = { ...current, ...params };
    set({ compressor: next });
    saveLocalConfig({ compressor: next });

    if (compressorDebounceTimer) {
      clearTimeout(compressorDebounceTimer);
    }

    return new Promise<boolean>((resolve) => {
      compressorDebounceTimer = setTimeout(async () => {
        compressorDebounceTimer = null;
        try {
          const call = routuneClient.setCompressor({
            channel: AudioChannel.MIC,
            config: next,
          });
          await call.response;
          resolve(true);
        } catch (err) {
          console.warn("Failed to set compressor:", err);
          resolve(false);
        }
      }, 50);
    });
  },

  setActiveView: (activeView) => set({ activeView }),

  setChannelVolume: (channel, volume) =>
    set((s) => ({
      channelVolumes: {
        ...s.channelVolumes,
        [channel]: Math.max(0, Math.min(100, volume)),
      },
    })),

  setChannelFaderDb: (channel, db) => {
    const clamped = Math.max(-48, Math.min(12, db));
    set((s) => {
      const nextFader = {
        ...s.channelFaderDb,
        [channel]: clamped,
      };
      saveLocalConfig({ channelFaderDb: nextFader });
      return {
        channelFaderDb: nextFader,
      };
    });
    dispatchChannelVolume(channel, clamped);
  },

  resetAllFaders: () => {
    const defaultFader = {
      [AudioChannel.MASTER]: 0.0,
      [AudioChannel.GAME]: 0.0,
      [AudioChannel.CHAT]: 0.0,
      [AudioChannel.MEDIA]: 0.0,
      [AudioChannel.AUX]: 0.0,
      [AudioChannel.MIC]: 0.0,
    };
    const defaultMute = {
      [AudioChannel.MASTER]: false,
      [AudioChannel.GAME]: false,
      [AudioChannel.CHAT]: false,
      [AudioChannel.MEDIA]: false,
      [AudioChannel.AUX]: false,
      [AudioChannel.MIC]: false,
    };
    saveLocalConfig({ channelFaderDb: defaultFader, channelMuted: defaultMute });
    set((s) => ({
      channelFaderDb: defaultFader,
      channelMuted: defaultMute,
      channelVolumes: {
        ...s.channelVolumes,
        [AudioChannel.MASTER]: 80,
        [AudioChannel.GAME]: 80,
        [AudioChannel.CHAT]: 80,
        [AudioChannel.MEDIA]: 80,
        [AudioChannel.AUX]: 80,
        [AudioChannel.MIC]: 80,
      },
    }));
    for (const ch of [
      AudioChannel.MASTER,
      AudioChannel.GAME,
      AudioChannel.CHAT,
      AudioChannel.MEDIA,
      AudioChannel.AUX,
      AudioChannel.MIC,
    ]) {
      dispatchChannelVolume(ch, 0.0);
      dispatchChannelMute(ch, false);
    }
  },

  toggleChannelMute: (channel) => {
    const currentMuted = useAudioStore.getState().channelMuted[channel] ?? false;
    const nextMuted = !currentMuted;
    set((s) => {
      const nextMutedMap = {
        ...s.channelMuted,
        [channel]: nextMuted,
      };
      saveLocalConfig({ channelMuted: nextMutedMap });
      return {
        channelMuted: nextMutedMap,
      };
    });
    dispatchChannelMute(channel, nextMuted);
  },
}));

// Discrete selectors to avoid unnecessary component tree re-renders
export const selectActiveView = (state: AudioStoreState) => state.activeView;
export const selectChannelVolumes = (state: AudioStoreState) => state.channelVolumes;
export const selectChannelMuted = (state: AudioStoreState) => state.channelMuted;
export const selectConnectionStatus = (state: AudioStoreState) => state.connectionStatus;
export const selectIsAudioActive = (state: AudioStoreState) => state.isAudioActive;
export const selectLatencyMs = (state: AudioStoreState) => state.latencyMs;
export const selectLastError = (state: AudioStoreState) => state.lastError;
export const selectTelemetry = (state: AudioStoreState) => ({
  peakDb: state.peakDb,
  rmsDb: state.rmsDb,
  sequenceNumber: state.sequenceNumber,
  channelMetrics: state.channelMetrics,
});
export const selectActiveApps = (state: AudioStoreState) => state.activeApps;
export const selectIsRoutingApp = (state: AudioStoreState) => state.isRoutingApp;
export const selectChannelNoiseEnabled = (state: AudioStoreState) => state.channelNoiseEnabled;
export const selectHardwareOutputs = (state: AudioStoreState) => state.hardwareOutputs;
export const selectHardwareInputs = (state: AudioStoreState) => state.hardwareInputs;
export const selectMicTestEnabled = (state: AudioStoreState) => state.micTestEnabled;
