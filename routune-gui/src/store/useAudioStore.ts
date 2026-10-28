import { create } from "zustand";

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

  // Actions
  setConnectionStatus: (status: ConnectionStatus, error?: string | null) => void;
  setAudioActive: (active: boolean) => void;
  setLatencyMs: (ms: number) => void;
  updateTelemetry: (peakDb: number, rmsDb: number, seq: string) => void;
  resetTelemetry: () => void;
}

export const useAudioStore = create<AudioStoreState>((set) => ({
  connectionStatus: "disconnected",
  isAudioActive: false,
  latencyMs: 0,
  lastError: null,

  peakDb: -100,
  rmsDb: -100,
  sequenceNumber: "0",

  setConnectionStatus: (status, error = null) =>
    set({
      connectionStatus: status,
      lastError: error,
      ...(status !== "connected" ? { isAudioActive: false, peakDb: -100, rmsDb: -100 } : {}),
    }),

  setAudioActive: (isAudioActive) => set({ isAudioActive }),

  setLatencyMs: (latencyMs) => set({ latencyMs }),

  updateTelemetry: (peakDb, rmsDb, sequenceNumber) =>
    set({
      peakDb,
      rmsDb,
      sequenceNumber,
    }),

  resetTelemetry: () =>
    set({
      peakDb: -100,
      rmsDb: -100,
      sequenceNumber: "0",
    }),
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
});
