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
}

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
