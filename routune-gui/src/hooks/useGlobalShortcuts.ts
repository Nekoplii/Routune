import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  register,
  unregisterAll,
} from "@tauri-apps/plugin-global-shortcut";
import { useSettingsStore } from "../store/useSettingsStore";
import { useAudioStore } from "../store/useAudioStore";
import { AudioChannel } from "../proto/routune";

// Converts stored UI format ("Ctrl+M") to Tauri accelerator format ("CmdOrCtrl+M")
function toTauriAccelerator(combo: string): string {
  return combo
    .split("+")
    .map((part) => {
      switch (part) {
        case "Ctrl":  return "CmdOrCtrl";
        case "Cmd":   return "CmdOrCtrl";
        case "Alt":   return "Alt";
        case "Shift": return "Shift";
        default:      return part;
      }
    })
    .join("+");
}

function normalizeKey(k: string): string {
  const upper = k.toUpperCase();
  if (upper === " " || upper === "SPACE") return "SPACE";
  return upper;
}

// Matches a browser KeyboardEvent against stored shortcut string (e.g. "Ctrl+M", "Ctrl+Shift+N")
function matchesKeyEvent(e: KeyboardEvent, combo: string): boolean {
  if (!combo || !combo.trim()) return false;
  const parts = combo.split("+").map((p) => p.trim());
  if (parts.length === 0) return false;

  const keyPart = normalizeKey(parts[parts.length - 1]);
  const hasCtrl = parts.includes("Ctrl");
  const hasCmd = parts.includes("Cmd");
  const hasAlt = parts.includes("Alt");
  const hasShift = parts.includes("Shift");

  const isCtrlOrCmd = e.ctrlKey || e.metaKey;
  const wantsCtrlOrCmd = hasCtrl || hasCmd;

  if (wantsCtrlOrCmd !== isCtrlOrCmd) return false;
  if (hasAlt !== e.altKey) return false;
  if (hasShift !== e.shiftKey) return false;

  const codeKey = e.code.startsWith("Key")
    ? e.code.slice(3).toUpperCase()
    : e.code.startsWith("Digit")
    ? e.code.slice(5).toUpperCase()
    : normalizeKey(e.code);

  const rawKey = normalizeKey(e.key);

  return keyPart === codeKey || keyPart === rawKey;
}

export type ShortcutStatus = {
  key: string;
  accelerator: string;
  ok: boolean;
  error?: string;
};

// Shared status so Settings can subscribe
let _statusListeners: Array<(s: ShortcutStatus[]) => void> = [];
let _lastStatus: ShortcutStatus[] = [];

export function useShortcutStatus(): ShortcutStatus[] {
  const [status, setStatus] = useState<ShortcutStatus[]>(_lastStatus);
  useEffect(() => {
    setStatus(_lastStatus);
    _statusListeners.push(setStatus);
    return () => { _statusListeners = _statusListeners.filter((l) => l !== setStatus); };
  }, []);
  return status;
}

function publishStatus(s: ShortcutStatus[]) {
  _lastStatus = s;
  _statusListeners.forEach((l) => l(s));
}

export function useGlobalShortcuts() {
  const muteMic    = useSettingsStore((s) => s.shortcuts.muteMic);
  const muteMaster = useSettingsStore((s) => s.shortcuts.muteMaster);
  const muteGame   = useSettingsStore((s) => s.shortcuts.muteGame);
  const muteChat   = useSettingsStore((s) => s.shortcuts.muteChat);
  const muteMedia  = useSettingsStore((s) => s.shortcuts.muteMedia);
  const muteAux    = useSettingsStore((s) => s.shortcuts.muteAux);

  useEffect(() => {
    let cancelled = false;
    const lastTriggered: Record<string, number> = {};

    const runAction = (key: string, act: () => void) => {
      const now = Date.now();
      if (lastTriggered[key] && now - lastTriggered[key] < 300) return;
      lastTriggered[key] = now;
      act();
    };

    const entries = [
      {
        key: "muteMic",
        combo: muteMic,
        action: () => useAudioStore.getState().toggleChannelMute(AudioChannel.MIC),
      },
      {
        key: "muteMaster",
        combo: muteMaster,
        action: () => useAudioStore.getState().toggleChannelMute(AudioChannel.MASTER),
      },
      {
        key: "muteGame",
        combo: muteGame,
        action: () => useAudioStore.getState().toggleChannelMute(AudioChannel.GAME),
      },
      {
        key: "muteChat",
        combo: muteChat,
        action: () => useAudioStore.getState().toggleChannelMute(AudioChannel.CHAT),
      },
      {
        key: "muteMedia",
        combo: muteMedia,
        action: () => useAudioStore.getState().toggleChannelMute(AudioChannel.MEDIA),
      },
      {
        key: "muteAux",
        combo: muteAux,
        action: () => useAudioStore.getState().toggleChannelMute(AudioChannel.AUX),
      },
    ];

    // 1. In-window shortcut listener (ensures shortcuts always work while Routune is focused on Wayland/X11)
    const onKeyDown = (e: KeyboardEvent) => {
      if (document.body.dataset.recordingShortcut) return;

      const target = e.target as HTMLElement | null;
      if (
        target &&
        (target.tagName === "INPUT" ||
          target.tagName === "TEXTAREA" ||
          target.isContentEditable)
      ) {
        return;
      }

      for (const { key, combo, action } of entries) {
        if (!combo.trim()) continue;
        if (matchesKeyEvent(e, combo)) {
          e.preventDefault();
          e.stopPropagation();
          runAction(key, action);
          return;
        }
      }
    };

    window.addEventListener("keydown", onKeyDown);

    // 2. Tauri system-wide shortcut registration (for X11 and platforms where XGrabKey is supported)
    (async () => {
      // Sync with Hyprland on Wayland so shortcuts work even when Routune is in background / unfocused
      try {
        await invoke("sync_hyprland_shortcuts", {
          muteMic,
          muteMaster,
          muteGame,
          muteChat,
          muteMedia,
          muteAux,
        });
      } catch (err) {
        console.warn("[shortcuts] Failed to sync Hyprland global shortcuts:", err);
      }

      // Flush any stale X11 grabs from previous effect run (StrictMode or re-mount)
      try { await unregisterAll(); } catch {}

      const results: ShortcutStatus[] = [];
      for (const { key, combo, action } of entries) {
        if (cancelled) break;
        if (!combo.trim()) {
          results.push({ key, accelerator: "(unbound)", ok: true });
          continue;
        }
        const accelerator = toTauriAccelerator(combo);
        try {
          await register(accelerator, (event) => {
            if (event.state === "Pressed") {
              runAction(key, action);
            }
          });
          if (!cancelled) results.push({ key, accelerator, ok: true });
        } catch (err) {
          const msg = err instanceof Error ? err.message : String(err);
          console.error(`[shortcuts] register("${accelerator}") failed:`, err);
          results.push({ key, accelerator, ok: false, error: msg });
        }
      }
      if (!cancelled) publishStatus(results);
    })();

    return () => {
      cancelled = true;
      window.removeEventListener("keydown", onKeyDown);
      unregisterAll().catch(() => {});
    };
  }, [muteMic, muteMaster, muteGame, muteChat, muteMedia, muteAux]);
}
