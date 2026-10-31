import { Minus, Square, Copy, X } from "lucide-react";
import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";

export type OsPlatform = "macos" | "windows" | "linux";

function detectPlatform(): OsPlatform {
  if (typeof navigator === "undefined") return "linux";
  const ua = navigator.userAgent.toLowerCase();
  const platform = (
    (navigator as unknown as { userAgentData?: { platform?: string } }).userAgentData?.platform ||
    navigator.platform ||
    ""
  ).toLowerCase();

  if (platform.includes("mac") || ua.includes("macintosh") || ua.includes("mac os x")) {
    return "macos";
  }
  if (platform.includes("win") || ua.includes("windows")) {
    return "windows";
  }
  return "linux";
}

export function Header() {
  const [isMaximized, setIsMaximized] = useState(false);
  const [os, setOs] = useState<OsPlatform>(detectPlatform);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    const init = async () => {
      try {
        if (typeof window !== "undefined" && "__TAURI_INTERNALS__" in window) {
          try {
            const detectedOs = await invoke<string>("get_platform");
            if (detectedOs === "macos" || detectedOs === "windows" || detectedOs === "linux") {
              setOs(detectedOs);
            }
          } catch {
            setOs(detectPlatform());
          }

          try {
            const max = await invoke<boolean>("app_is_maximized");
            setIsMaximized(Boolean(max));
          } catch {
            const { getCurrentWindow } = await import("@tauri-apps/api/window");
            setIsMaximized(await getCurrentWindow().isMaximized());
          }

          const { getCurrentWindow } = await import("@tauri-apps/api/window");
          const appWindow = getCurrentWindow();
          unlisten = await appWindow.onResized(async () => {
            try {
              const currentMax = await invoke<boolean>("app_is_maximized");
              setIsMaximized(Boolean(currentMax));
            } catch {
              setIsMaximized(await appWindow.isMaximized());
            }
          });
        }
      } catch (e) {
        console.warn("Failed to check window state:", e);
      }
    };
    init();
    return () => {
      if (unlisten) unlisten();
    };
  }, []);

  const handleMinimize = async () => {
    try {
      if (typeof window !== "undefined" && "__TAURI_INTERNALS__" in window) {
        await invoke("app_minimize");
      }
    } catch (e) {
      console.warn("Minimize failed via invoke, trying fallback:", e);
      try {
        const { getCurrentWindow } = await import("@tauri-apps/api/window");
        await getCurrentWindow().minimize();
      } catch {}
    }
  };

  const handleToggleMaximize = async () => {
    try {
      if (typeof window !== "undefined" && "__TAURI_INTERNALS__" in window) {
        const nextState = await invoke<boolean>("app_toggle_maximize");
        setIsMaximized(Boolean(nextState));
      }
    } catch (e) {
      console.warn("Maximize toggle failed via invoke, trying fallback:", e);
      try {
        const { getCurrentWindow } = await import("@tauri-apps/api/window");
        await getCurrentWindow().toggleMaximize();
        const max = await getCurrentWindow().isMaximized();
        setIsMaximized(max);
      } catch {}
    }
  };

  const handleClose = async () => {
    try {
      if (typeof window !== "undefined" && "__TAURI_INTERNALS__" in window) {
        await invoke("app_close");
      }
    } catch (e) {
      console.warn("Close failed via invoke, trying fallback:", e);
      try {
        const { getCurrentWindow } = await import("@tauri-apps/api/window");
        await getCurrentWindow().close();
      } catch {}
    }
  };

  const handleHeaderDoubleClick = (e: React.MouseEvent) => {
    const target = e.target as HTMLElement;
    if (target === e.currentTarget || target.hasAttribute("data-tauri-drag-region")) {
      handleToggleMaximize();
    }
  };

  return (
    <header
      data-tauri-drag-region
      onDoubleClick={handleHeaderDoubleClick}
      className="flex h-9 shrink-0 items-center justify-between border-b-2 border-white/10 bg-[#19191e] select-none cursor-default z-50"
    >
      {/* LEFT SECTION */}
      {os === "macos" ? (
        /* macOS Traffic Light Buttons (Top-Left) */
        <div
          className="group/mac flex items-center gap-2 pl-3.5 h-full"
          onPointerDown={(e) => e.stopPropagation()}
          onDoubleClick={(e) => e.stopPropagation()}
        >
          <button
            type="button"
            onClick={handleClose}
            className="relative flex h-3 w-3 items-center justify-center rounded-full bg-[#ff5f56] border border-[#e0443e] cursor-pointer hover:brightness-95 active:brightness-90 transition-all"
            title="Close"
            aria-label="Close"
          >
            <span className="opacity-0 group-hover/mac:opacity-100 text-[8px] font-bold text-black/75 leading-none select-none transition-opacity">
              ✕
            </span>
          </button>
          <button
            type="button"
            onClick={handleMinimize}
            className="relative flex h-3 w-3 items-center justify-center rounded-full bg-[#ffbd2e] border border-[#dea123] cursor-pointer hover:brightness-95 active:brightness-90 transition-all"
            title="Minimize"
            aria-label="Minimize"
          >
            <span className="opacity-0 group-hover/mac:opacity-100 text-[8px] font-bold text-black/75 leading-none select-none transition-opacity">
              –
            </span>
          </button>
          <button
            type="button"
            onClick={handleToggleMaximize}
            className="relative flex h-3 w-3 items-center justify-center rounded-full bg-[#27c93f] border border-[#1aab29] cursor-pointer hover:brightness-95 active:brightness-90 transition-all"
            title={isMaximized ? "Restore" : "Zoom"}
            aria-label={isMaximized ? "Restore" : "Zoom"}
          >
            <span className="opacity-0 group-hover/mac:opacity-100 text-[7px] font-bold text-black/75 leading-none select-none transition-opacity">
              +
            </span>
          </button>
        </div>
      ) : (
        /* Windows & Linux Left Brand Title */
        <div
          data-tauri-drag-region
          className="flex items-center gap-2 pl-4 text-xs font-semibold text-zinc-400 select-none h-full"
        >
          <span data-tauri-drag-region className="tracking-wide">
            Routune Studio
          </span>
        </div>
      )}

      {/* CENTER DRAGGABLE REGION */}
      <div
        data-tauri-drag-region
        className="flex-1 h-full flex items-center justify-center"
      >
        {os === "macos" && (
          <span
            data-tauri-drag-region
            className="text-[11px] font-semibold text-zinc-400/80 tracking-wide select-none pointer-events-none"
          >
            Routune Studio
          </span>
        )}
      </div>

      {/* RIGHT SECTION */}
      {os === "macos" ? (
        /* macOS has clean right side */
        <div data-tauri-drag-region className="w-16 h-full" />
      ) : os === "windows" ? (
        /* Windows 10/11 Chrome/Brave Caption Buttons (Top-Right, Flush Corner) */
        <div
          className="flex items-center h-full"
          onPointerDown={(e) => e.stopPropagation()}
          onDoubleClick={(e) => e.stopPropagation()}
        >
          <button
            type="button"
            onClick={handleMinimize}
            className="flex h-full w-11 items-center justify-center text-zinc-400 hover:bg-white/10 active:bg-white/15 hover:text-white transition-colors cursor-pointer"
            title="Minimize"
            aria-label="Minimize"
          >
            <svg width="10" height="1" viewBox="0 0 10 1" fill="currentColor">
              <rect width="10" height="1" />
            </svg>
          </button>

          <button
            type="button"
            onClick={handleToggleMaximize}
            className="flex h-full w-11 items-center justify-center text-zinc-400 hover:bg-white/10 active:bg-white/15 hover:text-white transition-colors cursor-pointer"
            title={isMaximized ? "Restore" : "Maximize"}
            aria-label={isMaximized ? "Restore" : "Maximize"}
          >
            {isMaximized ? (
              <svg
                width="10"
                height="10"
                viewBox="0 0 10 10"
                fill="none"
                stroke="currentColor"
                strokeWidth="1"
              >
                <path d="M2.5 7.5H1.5V1.5H7.5V2.5" />
                <rect x="2.5" y="2.5" width="6" height="6" />
              </svg>
            ) : (
              <svg
                width="10"
                height="10"
                viewBox="0 0 10 10"
                fill="none"
                stroke="currentColor"
                strokeWidth="1"
              >
                <rect x="1" y="1" width="8" height="8" />
              </svg>
            )}
          </button>

          <button
            type="button"
            onClick={handleClose}
            className="flex h-full w-11 items-center justify-center text-zinc-400 hover:bg-[#c42b1c] active:bg-[#b82618] hover:text-white transition-colors cursor-pointer"
            title="Close"
            aria-label="Close"
          >
            <svg
              width="10"
              height="10"
              viewBox="0 0 10 10"
              stroke="currentColor"
              strokeWidth="1.2"
            >
              <line x1="1" y1="1" x2="9" y2="9" />
              <line x1="9" y1="1" x2="1" y2="9" />
            </svg>
          </button>
        </div>
      ) : (
        /* Linux GNOME/Adwaita/KDE Chrome Action Buttons (Top-Right) */
        <div
          className="flex items-center gap-1.5 pr-3 h-full"
          onPointerDown={(e) => e.stopPropagation()}
          onDoubleClick={(e) => e.stopPropagation()}
        >
          <button
            type="button"
            onClick={handleMinimize}
            className="flex h-6 w-6 items-center justify-center rounded-full bg-white/5 hover:bg-white/15 text-zinc-300 hover:text-white transition-colors cursor-pointer"
            title="Minimize"
            aria-label="Minimize"
          >
            <Minus className="h-3 w-3" />
          </button>

          <button
            type="button"
            onClick={handleToggleMaximize}
            className="flex h-6 w-6 items-center justify-center rounded-full bg-white/5 hover:bg-white/15 text-zinc-300 hover:text-white transition-colors cursor-pointer"
            title={isMaximized ? "Restore" : "Maximize"}
            aria-label={isMaximized ? "Restore" : "Maximize"}
          >
            {isMaximized ? (
              <Copy className="h-2.5 w-2.5" />
            ) : (
              <Square className="h-2.5 w-2.5" />
            )}
          </button>

          <button
            type="button"
            onClick={handleClose}
            className="flex h-6 w-6 items-center justify-center rounded-full bg-white/5 hover:bg-rose-600 text-zinc-300 hover:text-white transition-colors cursor-pointer"
            title="Close"
            aria-label="Close"
          >
            <X className="h-3 w-3" />
          </button>
        </div>
      )}
    </header>
  );
}
