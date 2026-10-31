import { create } from "zustand";
import type { AppStream, ChannelVolumeMetric, OutputDeviceInfo } from "../proto/routune";
import { AudioChannel } from "../proto/routune";
import { routuneClient } from "../services/grpcClient";

export type ConnectionStatus = "disconnected" | "connecting" | "connected" | "error";

interface AudioStoreState {
  connectionStatus: ConnectionStatus;
  isAudioActive: boolean;
  latencyMs: number;
  lastError: string | null;

  // Real-time telemetry (60 FPS stream)
  peakDb: number;
  rmsDb: number;
  sequenceNumber: string;
  channelMetrics: ChannelVolumeMetric[];

  // Hardware routing (Sonar model)
  hardwareOutputs: OutputDeviceInfo[];
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
  fetchActiveApps: () => Promise<void>;
  routeApplication: (appId: number, targetChannel: AudioChannel) => Promise<boolean>;
  setChannelRouting: (channel: AudioChannel, deviceName: string) => Promise<boolean>;
  setSelectedEqChannel: (channel: AudioChannel) => void;
  setEqBandGain: (channel: AudioChannel, bandIndex: number, gainDb: number) => void;
  resetChannelEq: (channel: AudioChannel) => void;
  toggleAiNoise: (channel: AudioChannel, enabled: boolean) => Promise<boolean>;
}

const DEFAULT_EQ_BANDS = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0];

export const useAudioStore = create<AudioStoreState>((set) => ({
  connectionStatus: "disconnected",
  isAudioActive: false,
  latencyMs: 0,
  lastError: null,

  peakDb: -100,
  rmsDb: -100,
  sequenceNumber: "0",
  channelMetrics: [],

  hardwareOutputs: [],
  channelRoutings: {
    [AudioChannel.GAME]: "none",
    [AudioChannel.CHAT]: "none",
    [AudioChannel.MEDIA]: "none",
    [AudioChannel.AUX]: "none",
  },
  isRoutingUpdating: {},

  activeApps: [],
  isRoutingApp: {},

  selectedEqChannel: AudioChannel.GAME,
  eqBands: {
    [AudioChannel.GAME]: [...DEFAULT_EQ_BANDS],
    [AudioChannel.CHAT]: [...DEFAULT_EQ_BANDS],
    [AudioChannel.MEDIA]: [...DEFAULT_EQ_BANDS],
    [AudioChannel.AUX]: [...DEFAULT_EQ_BANDS],
  },

  channelNoiseEnabled: {
    [AudioChannel.GAME]: false,
    [AudioChannel.CHAT]: false,
    [AudioChannel.MEDIA]: false,
    [AudioChannel.AUX]: false,
  },
  isTogglingAiNoise: {},

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
      return {
        peakDb,
        rmsDb,
        sequenceNumber,
        channelMetrics,
        channelNoiseEnabled: updatedNoise,
      };
    }),

  resetTelemetry: () =>
    set({
      peakDb: -100,
      rmsDb: -100,
      sequenceNumber: "0",
      channelMetrics: [],
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
        set((s) => ({
          channelRoutings: { ...s.channelRoutings, [channel]: res.activeDeviceName },
          isRoutingUpdating: { ...s.isRoutingUpdating, [channel]: false },
        }));
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
      return {
        eqBands: {
          ...s.eqBands,
          [channel]: currentBands,
        },
      };
    }),

  resetChannelEq: (channel) =>
    set((s) => ({
      eqBands: {
        ...s.eqBands,
        [channel]: [...DEFAULT_EQ_BANDS],
      },
    })),

  toggleAiNoise: async (channel: AudioChannel, enabled: boolean) => {
    const prevEnabled = useAudioStore.getState().channelNoiseEnabled[channel] ?? false;
    // Optimistic update
    set((s) => ({
      isTogglingAiNoise: { ...s.isTogglingAiNoise, [channel]: true },
      channelNoiseEnabled: { ...s.channelNoiseEnabled, [channel]: enabled },
    }));

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
      set((s) => ({
        channelNoiseEnabled: { ...s.channelNoiseEnabled, [channel]: prevEnabled },
        isTogglingAiNoise: { ...s.isTogglingAiNoise, [channel]: false },
      }));
      return false;
    }
  },
}));

// Discrete selectors to avoid unnecessary component tree re-renders
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
