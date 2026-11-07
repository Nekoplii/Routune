import {
  SlidersHorizontal,
  Split,
  AudioWaveform,
  Settings as SettingsIcon,
  Info,
} from "lucide-react";
import { useAudioStore } from "../store/useAudioStore";
import type { NavigationView } from "../store/useAudioStore";

interface NavItem {
  id: NavigationView;
  label: string;
  icon: typeof SlidersHorizontal;
}

const NAV_ITEMS: NavItem[] = [
  {
    id: "mixer",
    label: "Mixer",
    icon: SlidersHorizontal,
  },
  {
    id: "routing",
    label: "Routing",
    icon: Split,
  },
  {
    id: "tuning",
    label: "Tuning",
    icon: AudioWaveform,
  },
  {
    id: "settings",
    label: "Settings",
    icon: SettingsIcon,
  },
  {
    id: "about",
    label: "About",
    icon: Info,
  },
];

export function Sidebar() {
  const activeView = useAudioStore((s) => s.activeView);
  const setActiveView = useAudioStore((s) => s.setActiveView);

  return (
    <aside className="w-56 shrink-0 flex flex-col justify-between border-r-2 border-[#2B2942] bg-[#171528] p-4 select-none">
      {/* Top Brand Logo & Navigation */}
      <div className="flex flex-col gap-6">
        <div className="flex items-center gap-3 px-2 pt-2">
          {/* Brand logo icon */}
          <div className="flex h-10 w-10 items-center justify-center rounded-xl bg-[#7C6AF2]/15 border-2 border-[#7C6AF2]/30 p-1">
            <img src="/logo.svg" alt="Routune" className="h-full w-full object-contain pointer-events-none" />
          </div>
          <span className="text-lg font-bold tracking-tight text-[#ECEAFB] font-sans">
            Routune
          </span>
        </div>

        {/* Navigation Section */}
        <nav className="flex flex-col gap-1.5">
          {NAV_ITEMS.map((item) => {
            const Icon = item.icon;
            const isActive = activeView === item.id;
            return (
              <button
                type="button"
                key={item.id}
                onClick={() => setActiveView(item.id)}
                className={`group flex items-center gap-3.5 rounded-xl px-4 py-3 text-sm font-medium transition-colors cursor-pointer border-2 ${
                  isActive
                    ? "bg-[#7C6AF2] border-[#7C6AF2] text-white shadow-md shadow-[#7C6AF2]/25"
                    : "border-transparent text-[#ECEAFB]/70 hover:bg-[#201D36] hover:border-[#2B2942] hover:text-[#ECEAFB]"
                }`}
              >
                <Icon
                  className={`h-4 w-4 ${
                    isActive ? "text-white" : "text-[#ECEAFB]/60 group-hover:text-[#C4B5FD]"
                  }`}
                />
                <span>{item.label}</span>
              </button>
            );
          })}
        </nav>
      </div>
    </aside>
  );
}
