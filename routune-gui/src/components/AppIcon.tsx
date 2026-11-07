import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Volume2 } from "lucide-react";

interface AppIconProps {
  name: string;
  iconName?: string;
  className?: string;
}

// In-memory cache for resolved data URIs to avoid repeated IPC lookups
const iconCache: Record<string, string | null> = {};

export function AppIcon({ name, iconName = "", className = "h-5 w-5" }: AppIconProps) {
  const [iconSrc, setIconSrc] = useState<string | null>(() => {
    const key = `${name}::${iconName}`.toLowerCase();
    return iconCache[key] ?? null;
  });

  useEffect(() => {
    const key = `${name}::${iconName}`.toLowerCase();
    if (iconCache[key] !== undefined) {
      setIconSrc(iconCache[key]);
      return;
    }

    let isMounted = true;
    if (typeof window !== "undefined" && "__TAURI_INTERNALS__" in window) {
      invoke<string | null>("get_app_icon", {
        iconName: iconName || name,
        appName: name,
      })
        .then((res) => {
          iconCache[key] = res;
          if (isMounted) {
            setIconSrc(res);
          }
        })
        .catch(() => {
          iconCache[key] = null;
        });
    } else {
      iconCache[key] = null;
    }

    return () => {
      isMounted = false;
    };
  }, [name, iconName]);

  // If system icon resolved as data URI, render native image
  if (iconSrc) {
    return (
      <img
        src={iconSrc}
        alt={name}
        className={`${className} object-contain rounded-md select-none pointer-events-none`}
        onError={() => setIconSrc(null)}
      />
    );
  }

  // Crisp Vector Fallback Icons
  const query = `${name} ${iconName}`.toLowerCase();

  // Discord
  if (query.includes("discord")) {
    return (
      <svg viewBox="0 0 24 24" fill="none" className={className}>
        <rect width="24" height="24" rx="6" fill="#5865F2" />
        <path
          d="M18.8 6.5C17.5 5.9 16.1 5.5 14.6 5.3C14.4 5.7 14.2 6.1 14.1 6.5C12.5 6.2 10.9 6.2 9.3 6.5C9.1 6.1 8.9 5.7 8.7 5.3C7.2 5.5 5.8 5.9 4.5 6.5C2 10.3 1.3 14 1.7 17.6C3.4 18.9 5 19.7 6.6 20.2C7 19.6 7.4 19 7.7 18.3C7.1 18.1 6.6 17.8 6.1 17.5C6.2 17.4 6.3 17.3 6.4 17.2C9.7 18.7 13.3 18.7 16.6 17.2C16.7 17.3 16.8 17.4 17 17.5C16.4 17.8 15.9 18.1 15.4 18.3C15.7 19 16.1 19.6 16.5 20.2C18.1 19.7 19.7 18.9 21.4 17.6C21.8 13.4 20.8 9.7 18.8 6.5ZM8.5 15.2C7.5 15.2 6.7 14.3 6.7 13.2C6.7 12.1 7.5 11.2 8.5 11.2C9.5 11.2 10.3 12.1 10.3 13.2C10.3 14.3 9.5 15.2 8.5 15.2ZM14.8 15.2C13.8 15.2 13 14.3 13 13.2C13 12.1 13.8 11.2 14.8 11.2C15.8 11.2 16.6 12.1 16.6 13.2C16.6 14.3 15.8 15.2 14.8 15.2Z"
          fill="white"
        />
      </svg>
    );
  }

  // Spotify
  if (query.includes("spotify")) {
    return (
      <svg viewBox="0 0 24 24" fill="none" className={className}>
        <circle cx="12" cy="12" r="11" fill="#1DB954" />
        <path
          d="M16.5 16.2C16.3 16.5 15.9 16.6 15.6 16.4C13.1 14.9 9.9 14.5 6.4 15.3C6 15.4 5.7 15.2 5.6 14.8C5.5 14.4 5.7 14.1 6.1 14C9.9 13.1 13.5 13.5 16.3 15.3C16.6 15.4 16.7 15.8 16.5 16.2ZM17.8 13.5C17.5 13.9 17 14.1 16.6 13.8C13.8 12.1 9.6 11.6 6.3 12.6C5.9 12.7 5.4 12.5 5.3 12.1C5.2 11.7 5.4 11.2 5.8 11.1C9.6 9.9 14.3 10.5 17.5 12.5C17.9 12.6 18 13.1 17.8 13.5ZM18 10.7C14.7 8.7 9.2 8.5 5.9 9.5C5.4 9.7 4.8 9.4 4.7 8.9C4.5 8.4 4.8 7.8 5.3 7.7C9.1 6.5 15.2 6.7 19.1 9C19.5 9.2 19.7 9.8 19.4 10.3C19.1 10.7 18.5 10.9 18 10.7Z"
          fill="#121212"
        />
      </svg>
    );
  }

  // Counter-Strike / CS2 / Steam Games
  if (query.includes("cs2") || query.includes("strike") || query.includes("counter")) {
    return (
      <svg viewBox="0 0 24 24" fill="none" className={className}>
        <rect width="24" height="24" rx="6" fill="#1b2838" />
        <rect x="1" y="1" width="22" height="22" rx="5" stroke="#de9b35" strokeWidth="1" />
        <path
          d="M7 6H17V9H10V15H17V18H7V6Z"
          fill="#de9b35"
        />
        <text
          x="12"
          y="15"
          textAnchor="middle"
          fill="#ffffff"
          fontSize="7"
          fontWeight="900"
          fontFamily="monospace"
        >
          CS2
        </text>
      </svg>
    );
  }

  // Steam
  if (query.includes("steam")) {
    return (
      <svg viewBox="0 0 24 24" fill="none" className={className}>
        <rect width="24" height="24" rx="6" fill="#171a21" />
        <path
          d="M12 2C6.5 2 2 6.5 2 12C2 16.5 5 20.3 9.1 21.6L12.5 16.7C12.3 16.3 12.2 15.8 12.2 15.3C12.2 13.5 13.7 12 15.5 12C17.3 12 18.8 13.5 18.8 15.3C18.8 17.1 17.3 18.6 15.5 18.6C14.7 18.6 14 18.3 13.4 17.8L9.8 21.9C10.5 22 11.2 22 12 22C17.5 22 22 17.5 22 12C22 6.5 17.5 2 12 2Z"
          fill="#c7d5e0"
        />
        <circle cx="15.5" cy="15.3" r="2" fill="#171a21" />
        <circle cx="8" cy="17" r="1.5" fill="#c7d5e0" />
      </svg>
    );
  }

  // Firefox
  if (query.includes("firefox")) {
    return (
      <svg viewBox="0 0 24 24" fill="none" className={className}>
        <rect width="24" height="24" rx="6" fill="#ff7139" />
        <circle cx="12" cy="12" r="7" fill="#0060df" />
        <path
          d="M17.5 7.5C16.5 6.5 15.2 6 13.8 6C10.5 6 7.8 8.7 7.8 12C7.8 15.3 10.5 18 13.8 18C16.8 18 19.3 15.8 19.7 12.9C19.8 12.4 19.4 12 18.9 12C18.5 12 18.2 12.3 18.1 12.7C17.7 14.6 16 16.1 13.9 16.1C11.6 16.1 9.8 14.3 9.8 12C9.8 9.7 11.6 7.9 13.9 7.9C14.8 7.9 15.7 8.2 16.4 8.8C16.9 9.2 17.6 9.1 17.9 8.6C18.1 8.2 18 7.7 17.5 7.5Z"
          fill="#ffe900"
        />
      </svg>
    );
  }

  // Google Chrome / Chromium
  if (query.includes("chrome") || query.includes("chromium")) {
    return (
      <svg viewBox="0 0 24 24" fill="none" className={className}>
        <circle cx="12" cy="12" r="11" fill="#ea4335" />
        <circle cx="12" cy="12" r="5" fill="#ffffff" />
        <circle cx="12" cy="12" r="4" fill="#1a73e8" />
        <path d="M12 2C15.3 2 18.2 3.6 20 6.1L14.5 15.6H9L12 2Z" fill="#ea4335" />
        <path d="M2 12C2 8.7 3.6 5.8 6.1 4L11.6 13.5V19L2 12Z" fill="#34a853" />
        <path d="M12 22C8.7 22 5.8 20.4 4 17.9L9.5 8.4H15L12 22Z" fill="#fbbc05" />
      </svg>
    );
  }

  // Brave
  if (query.includes("brave")) {
    return (
      <svg viewBox="0 0 24 24" fill="none" className={className}>
        <rect width="24" height="24" rx="6" fill="#fb542b" />
        <path
          d="M12 4L16 6V11C16 15 12 19 12 19C12 19 8 15 8 11V6L12 4Z"
          fill="white"
        />
      </svg>
    );
  }

  // VLC
  if (query.includes("vlc")) {
    return (
      <svg viewBox="0 0 24 24" fill="none" className={className}>
        <rect width="24" height="24" rx="6" fill="#e05d06" />
        <polygon points="12,4 16,18 8,18" fill="#ffffff" />
        <polygon points="12,7 14.5,15 9.5,15" fill="#e05d06" />
        <rect x="7" y="18" width="10" height="2" rx="1" fill="#ffffff" />
      </svg>
    );
  }

  // OBS Studio
  if (query.includes("obs")) {
    return (
      <svg viewBox="0 0 24 24" fill="none" className={className}>
        <circle cx="12" cy="12" r="11" fill="#1f1f24" />
        <circle cx="12" cy="12" r="8" stroke="#302e38" strokeWidth="2" />
        <circle cx="12" cy="12" r="3" fill="#ffffff" />
      </svg>
    );
  }

  // System Sounds
  if (query.includes("system") || query.includes("sound") || query.includes("alsa")) {
    return (
      <svg viewBox="0 0 24 24" fill="none" className={className}>
        <rect width="24" height="24" rx="6" fill="#171528" stroke="#2B2942" strokeWidth="1" />
        <path
          d="M7 10V14H10L14 18V6L10 10H7Z"
          fill="#7C6AF2"
        />
        <path
          d="M16.5 8.5C17.5 9.5 18 10.7 18 12C18 13.3 17.5 14.5 16.5 15.5"
          stroke="#C4B5FD"
          strokeWidth="1.5"
          strokeLinecap="round"
        />
      </svg>
    );
  }

  // Generic Application Tile with dynamic initial
  const initial = (name.trim().charAt(0) || "A").toUpperCase();

  return (
    <div className={`relative flex items-center justify-center rounded-lg bg-[#171528] border border-[#2B2942] ${className}`}>
      <span className="text-[11px] font-bold text-[#ECEAFB] select-none font-mono">
        {initial}
      </span>
      <Volume2 className="absolute bottom-0.5 right-0.5 h-2 w-2 text-[#ECEAFB]/60 opacity-70" />
    </div>
  );
}
