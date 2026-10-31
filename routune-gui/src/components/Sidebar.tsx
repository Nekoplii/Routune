import {
  SlidersHorizontal,
  Split,
  AudioWaveform,
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
    id: "about",
    label: "About",
    icon: Info,
  },
];

export function Sidebar() {
  const activeView = useAudioStore((s) => s.activeView);
  const setActiveView = useAudioStore((s) => s.setActiveView);

  return (
    <aside className="w-56 shrink-0 flex flex-col justify-between border-r-2 border-white/10 bg-[#19191e] p-4 select-none">
      {/* Top Brand Logo & Navigation */}
      <div className="flex flex-col gap-6">
        <div className="flex items-center gap-3 px-2 pt-2">
          {/* Orange waveform icon */}
          <div className="flex h-10 w-10 items-center justify-center rounded-xl bg-[#ff5511]/15 border-2 border-[#ff5511]/30 text-[#ff5511]">
            <AudioWaveform className="h-5 w-5" />
          </div>
          <span className="text-lg font-bold tracking-tight text-white font-sans">
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
                    ? "bg-[#ff5511] border-[#ff5511] text-white"
                    : "border-transparent text-zinc-300 hover:bg-[#25252c] hover:border-white/10 hover:text-white"
                }`}
              >
                <Icon
                  className={`h-4 w-4 ${
                    isActive ? "text-white" : "text-zinc-400 group-hover:text-zinc-200"
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
