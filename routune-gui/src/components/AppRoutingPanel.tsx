import { useState, useEffect, useMemo } from "react";
import {
  DndContext,
  DragOverlay,
  PointerSensor,
  useSensor,
  useSensors,
  useDraggable,
  useDroppable,
} from "@dnd-kit/core";
import type { DragStartEvent, DragEndEvent } from "@dnd-kit/core";
import { CSS } from "@dnd-kit/utilities";
import {
  Gamepad2,
  MessageSquare,
  Music,
  Radio,
  Laptop,
  GripVertical,
  Volume2,
  Globe,
  Loader2,
  RefreshCw,
  Layers,
} from "lucide-react";
import { useAudioStore } from "../store/useAudioStore";
import { AudioChannel } from "../proto/routune";
import type { AppStream } from "../proto/routune";

interface ZoneConfig {
  channel: AudioChannel;
  title: string;
  subtitle: string;
  icon: typeof Laptop;
  headerColor: string;
  badgeBg: string;
  dropBorder: string;
  activeBg: string;
}

const ZONES: ZoneConfig[] = [
  {
    channel: AudioChannel.MASTER,
    title: "OS Default",
    subtitle: "Unassigned Streams",
    icon: Laptop,
    headerColor: "text-neutral-300",
    badgeBg: "bg-neutral-800 text-neutral-300 border-neutral-700",
    dropBorder: "border-neutral-500",
    activeBg: "bg-neutral-900/60",
  },
  {
    channel: AudioChannel.GAME,
    title: "Game",
    subtitle: "Routune Game Sink",
    icon: Gamepad2,
    headerColor: "text-emerald-400",
    badgeBg: "bg-emerald-500/10 text-emerald-400 border-emerald-500/20",
    dropBorder: "border-emerald-500",
    activeBg: "bg-emerald-950/20",
  },
  {
    channel: AudioChannel.CHAT,
    title: "Chat",
    subtitle: "Routune Chat Sink",
    icon: MessageSquare,
    headerColor: "text-sky-400",
    badgeBg: "bg-sky-500/10 text-sky-400 border-sky-500/20",
    dropBorder: "border-sky-500",
    activeBg: "bg-sky-950/20",
  },
  {
    channel: AudioChannel.MEDIA,
    title: "Media",
    subtitle: "Routune Media Sink",
    icon: Music,
    headerColor: "text-purple-400",
    badgeBg: "bg-purple-500/10 text-purple-400 border-purple-500/20",
    dropBorder: "border-purple-500",
    activeBg: "bg-purple-950/20",
  },
  {
    channel: AudioChannel.AUX,
    title: "Aux",
    subtitle: "Routune Aux Sink",
    icon: Radio,
    headerColor: "text-amber-400",
    badgeBg: "bg-amber-500/10 text-amber-400 border-amber-500/20",
    dropBorder: "border-amber-500",
    activeBg: "bg-amber-950/20",
  },
];

function getAppCategoryIcon(name: string, iconName: string) {
  const query = `${name} ${iconName}`.toLowerCase();
  if (
    query.includes("spotify") ||
    query.includes("music") ||
    query.includes("vlc") ||
    query.includes("mpv") ||
    query.includes("clementine") ||
    query.includes("audacious")
  ) {
    return Music;
  }
  if (
    query.includes("discord") ||
    query.includes("slack") ||
    query.includes("teams") ||
    query.includes("skype") ||
    query.includes("telegram") ||
    query.includes("zoom")
  ) {
    return MessageSquare;
  }
  if (
    query.includes("game") ||
    query.includes("steam") ||
    query.includes("heroic") ||
    query.includes("lutris") ||
    query.includes("wine") ||
    query.includes("retroarch")
  ) {
    return Gamepad2;
  }
  if (
    query.includes("firefox") ||
    query.includes("chrome") ||
    query.includes("chromium") ||
    query.includes("brave") ||
    query.includes("edge") ||
    query.includes("browser")
  ) {
    return Globe;
  }
  return Volume2;
}

interface DraggableAppCardProps {
  app: AppStream;
  isOverlay?: boolean;
}

function DraggableAppCard({ app, isOverlay = false }: DraggableAppCardProps) {
  const isRoutingApp = useAudioStore((s) => s.isRoutingApp[app.appId]);
  const { attributes, listeners, setNodeRef, transform, isDragging } = useDraggable({
    id: `app-${app.appId}`,
    data: { app },
  });

  const style = transform
    ? {
        transform: CSS.Translate.toString(transform),
      }
    : undefined;

  const AppIcon = getAppCategoryIcon(app.name, app.iconName);

  return (
    <div
      ref={setNodeRef}
      style={style}
      {...attributes}
      {...listeners}
      className={`group relative flex items-center gap-2.5 rounded-lg border px-3 py-2 text-xs transition-all select-none cursor-grab active:cursor-grabbing ${
        isOverlay
          ? "border-cyan-500/60 bg-neutral-900 shadow-2xl ring-2 ring-cyan-500/20 scale-105 z-50 text-neutral-100"
          : isDragging
          ? "opacity-30 border-dashed border-neutral-700 bg-neutral-950"
          : "border-neutral-800 bg-neutral-900/90 hover:border-neutral-700 hover:bg-neutral-850 hover:shadow-md text-neutral-200"
      }`}
    >
      <GripVertical className="h-3.5 w-3.5 flex-shrink-0 text-neutral-600 group-hover:text-neutral-400 transition-colors" />

      <div className="flex h-6 w-6 flex-shrink-0 items-center justify-center rounded bg-neutral-800 border border-neutral-700/60 text-neutral-400 group-hover:text-neutral-200">
        <AppIcon className="h-3.5 w-3.5" />
      </div>

      <div className="flex min-w-0 flex-1 flex-col">
        <span className="truncate font-semibold tracking-wide text-neutral-100 group-hover:text-white">
          {app.name || "Unknown App"}
        </span>
        <span className="truncate text-[10px] text-neutral-500 font-mono">
          node #{app.appId}
        </span>
      </div>

      {isRoutingApp && (
        <Loader2 className="h-3.5 w-3.5 flex-shrink-0 animate-spin text-cyan-400" />
      )}
    </div>
  );
}

interface DroppableZoneProps {
  zone: ZoneConfig;
  apps: AppStream[];
}

function DroppableZone({ zone, apps }: DroppableZoneProps) {
  const { isOver, setNodeRef } = useDroppable({
    id: `zone-${zone.channel}`,
    data: { channel: zone.channel },
  });

  const Icon = zone.icon;

  return (
    <div
      ref={setNodeRef}
      className={`flex flex-col rounded-xl border transition-all duration-200 min-h-[175px] ${
        isOver
          ? `border-2 ${zone.dropBorder} ${zone.activeBg} ring-2 ring-cyan-500/20 shadow-lg scale-[1.01]`
          : "border-neutral-800/80 bg-neutral-900/40 hover:border-neutral-750"
      }`}
    >
      {/* Zone Header */}
      <div className="flex items-center justify-between border-b border-neutral-800/60 px-3 py-2.5 bg-neutral-900/60 rounded-t-xl">
        <div className="flex items-center gap-2 min-w-0">
          <Icon className={`h-4 w-4 flex-shrink-0 ${zone.headerColor}`} />
          <div className="flex flex-col min-w-0">
            <span className="text-xs font-semibold uppercase tracking-wider text-neutral-200 truncate">
              {zone.title}
            </span>
            <span className="text-[10px] text-neutral-500 truncate">
              {zone.subtitle}
            </span>
          </div>
        </div>
        <span
          className={`flex h-5 min-w-[20px] items-center justify-center rounded-full px-1.5 text-[10px] font-mono font-medium border ${zone.badgeBg}`}
        >
          {apps.length}
        </span>
      </div>

      {/* Droppable Body */}
      <div className="flex-1 p-2 space-y-1.5 overflow-y-auto max-h-[220px]">
        {apps.length === 0 ? (
          <div
            className={`flex h-full min-h-[110px] flex-col items-center justify-center rounded-lg border border-dashed text-center p-3 transition-colors ${
              isOver
                ? "border-cyan-500/50 bg-cyan-950/10 text-cyan-300"
                : "border-neutral-850/80 text-neutral-600"
            }`}
          >
            <p className="text-[11px] font-medium">
              {isOver ? "Release to assign" : "Drop apps here"}
            </p>
            <p className="text-[9px] text-neutral-600 mt-0.5">
              Drag from any channel
            </p>
          </div>
        ) : (
          apps.map((app) => <DraggableAppCard key={app.appId} app={app} />)
        )}
      </div>
    </div>
  );
}

export function AppRoutingPanel() {
  const connectionStatus = useAudioStore((s) => s.connectionStatus);
  const activeApps = useAudioStore((s) => s.activeApps);
  const fetchActiveApps = useAudioStore((s) => s.fetchActiveApps);
  const routeApplication = useAudioStore((s) => s.routeApplication);

  const [activeDragApp, setActiveDragApp] = useState<AppStream | null>(null);
  const [isManualRefreshing, setIsManualRefreshing] = useState(false);

  const sensors = useSensors(
    useSensor(PointerSensor, {
      activationConstraint: {
        distance: 5,
      },
    })
  );

  // Poll active applications every 1.5 seconds when connected
  useEffect(() => {
    if (connectionStatus !== "connected") return;

    fetchActiveApps();
    const interval = setInterval(() => {
      fetchActiveApps();
    }, 1500);

    return () => clearInterval(interval);
  }, [connectionStatus, fetchActiveApps]);

  const handleManualRefresh = async () => {
    setIsManualRefreshing(true);
    await fetchActiveApps();
    setTimeout(() => setIsManualRefreshing(false), 500);
  };

  const handleDragStart = (event: DragStartEvent) => {
    const app = event.active.data.current?.app as AppStream | undefined;
    if (app) {
      setActiveDragApp(app);
    }
  };

  const handleDragEnd = (event: DragEndEvent) => {
    setActiveDragApp(null);
    const { active, over } = event;
    if (!over) return;

    const app = active.data.current?.app as AppStream | undefined;
    const targetChannel = over.data.current?.channel as AudioChannel | undefined;

    if (app && targetChannel !== undefined && app.currentSink !== targetChannel) {
      routeApplication(app.appId, targetChannel);
    }
  };

  // Group apps by currentSink (AudioChannel)
  const appsByChannel = useMemo(() => {
    const map: Record<number, AppStream[]> = {
      [AudioChannel.MASTER]: [],
      [AudioChannel.GAME]: [],
      [AudioChannel.CHAT]: [],
      [AudioChannel.MEDIA]: [],
      [AudioChannel.AUX]: [],
    };

    activeApps.forEach((app) => {
      if (map[app.currentSink]) {
        map[app.currentSink].push(app);
      } else {
        // Fallback default
        map[AudioChannel.MASTER].push(app);
      }
    });

    return map;
  }, [activeApps]);

  return (
    <div className="rounded-xl border border-neutral-800 bg-neutral-900/50 p-4 shadow-xl backdrop-blur-sm">
      {/* Header Bar */}
      <div className="mb-4 flex items-center justify-between">
        <div className="flex items-center gap-2">
          <Layers className="h-4 w-4 text-cyan-400" />
          <h2 className="text-xs font-semibold uppercase tracking-wider text-neutral-300">
            Sonar Application Routing
          </h2>
          <span className="rounded bg-neutral-800 px-2 py-0.5 text-[10px] font-mono text-neutral-400 border border-neutral-700/60">
            {activeApps.length} active {activeApps.length === 1 ? "stream" : "streams"}
          </span>
        </div>

        <div className="flex items-center gap-3">
          <span className="hidden sm:inline-block text-[11px] text-neutral-500">
            Drag apps between channels to reroute audio
          </span>
          <button
            onClick={handleManualRefresh}
            disabled={connectionStatus !== "connected" || isManualRefreshing}
            className="flex items-center gap-1.5 rounded-lg border border-neutral-700 bg-neutral-800/80 px-2.5 py-1 text-xs text-neutral-300 hover:border-neutral-600 hover:bg-neutral-750 transition-colors disabled:opacity-50"
            title="Refresh active applications"
          >
            <RefreshCw
              className={`h-3 w-3 ${isManualRefreshing ? "animate-spin text-cyan-400" : ""}`}
            />
            <span className="text-[11px]">Refresh</span>
          </button>
        </div>
      </div>

      {/* Drag & Drop Columns Grid */}
      <DndContext
        sensors={sensors}
        onDragStart={handleDragStart}
        onDragEnd={handleDragEnd}
        onDragCancel={() => setActiveDragApp(null)}
      >
        <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-5 gap-3">
          {ZONES.map((zone) => (
            <DroppableZone
              key={zone.channel}
              zone={zone}
              apps={appsByChannel[zone.channel] || []}
            />
          ))}
        </div>

        <DragOverlay dropAnimation={null}>
          {activeDragApp ? <DraggableAppCard app={activeDragApp} isOverlay /> : null}
        </DragOverlay>
      </DndContext>
    </div>
  );
}
