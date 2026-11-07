import { create } from "zustand";
import { persist } from "zustand/middleware";

export type LatencyMode = "ultra_low" | "balanced" | "stable";

export interface GlobalShortcuts {
  muteMic: string;
  muteMaster: string;
  muteGame: string;
  muteChat: string;
  muteMedia: string;
  muteAux: string;
}

export interface SettingsState {
  // System Behavior
  runOnStartup: boolean;
  startMinimized: boolean;
  closeToTray: boolean;
  autoCheckUpdates: boolean;

  // Audio Engine
  latencyMode: LatencyMode;

  // Global Shortcuts
  shortcuts: GlobalShortcuts;

  // Actions
  setRunOnStartup: (val: boolean) => void;
  setStartMinimized: (val: boolean) => void;
  setCloseToTray: (val: boolean) => void;
  setAutoCheckUpdates: (val: boolean) => void;
  setLatencyMode: (val: LatencyMode) => void;
  setShortcut: (key: keyof GlobalShortcuts, shortcut: string) => void;
  resetSettings: () => void;
}

const DEFAULT_SETTINGS = {
  runOnStartup: false,
  startMinimized: false,
  closeToTray: true,
  autoCheckUpdates: true,
  latencyMode: "balanced" as LatencyMode,
  shortcuts: {
    muteMic: "Ctrl+M",
    muteMaster: "Ctrl+Shift+M",
    muteGame: "",
    muteChat: "",
    muteMedia: "",
    muteAux: "",
  },
};

export const useSettingsStore = create<SettingsState>()(
  persist(
    (set) => ({
      ...DEFAULT_SETTINGS,

      setRunOnStartup: (runOnStartup) => set({ runOnStartup }),
      setStartMinimized: (startMinimized) => set({ startMinimized }),
      setCloseToTray: (closeToTray) => set({ closeToTray }),
      setAutoCheckUpdates: (autoCheckUpdates) => set({ autoCheckUpdates }),
      setLatencyMode: (latencyMode) => set({ latencyMode }),
      setShortcut: (key, shortcut) =>
        set((state) => ({
          shortcuts: {
            ...state.shortcuts,
            [key]: shortcut,
          },
        })),
      resetSettings: () => set({ ...DEFAULT_SETTINGS }),
    }),
    {
      name: "routune-app-settings",
    }
  )
);
