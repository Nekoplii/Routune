import { create } from "zustand";
import type { ChannelVolumeMetric, OutputDeviceInfo } from "../proto/routune";
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

  // 10-Band Parametric Equalizer State
  selectedEqChannel: AudioChannel;
  eqBands: Record<number, number[]>;

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
  setChannelRouting: (channel: AudioChannel, deviceName: string) => Promise<boolean>;
  setSelectedEqChannel: (channel: AudioChannel) => void;
  setEqBandGain: (channel: AudioChannel, bandIndex: number, gainDb: number) => void;
  resetChannelEq: (channel: AudioChannel) => void;
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

  selectedEqChannel: AudioChannel.GAME,
  eqBands: {
    [AudioChannel.GAME]: [...DEFAULT_EQ_BANDS],
    [AudioChannel.CHAT]: [...DEFAULT_EQ_BANDS],
    [AudioChannel.MEDIA]: [...DEFAULT_EQ_BANDS],
    [AudioChannel.AUX]: [...DEFAULT_EQ_BANDS],
  },

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
    set({
      peakDb,
      rmsDb,
      sequenceNumber,
      channelMetrics,
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
