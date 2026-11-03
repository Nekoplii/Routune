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
  Mic,
  Music,
  Radio,
  GripVertical,
  ChevronDown,
  RotateCw,
  ArrowDown,
  Loader2,
  Headphones,
} from "lucide-react";
import { useAudioStore } from "../store/useAudioStore";
import { AudioChannel } from "../proto/routune";
import type { AppStream } from "../proto/routune";
import { AppIcon } from "./AppIcon";

interface ChannelConfig {
  channel: AudioChannel;
  title: string;
  sub: string;
  icon: typeof Gamepad2;
  iconBg: string;
  iconBorder: string;
  iconColor: string;
  badgeBg: string;
}

const CHANNEL_CONFIGS: ChannelConfig[] = [
  {
    channel: AudioChannel.GAME,
    title: "Game",
    sub: "routune-game",
    icon: Gamepad2,
    iconBg: "bg-orange-500/15",
    iconBorder: "border-[#ff5511]/30",
    iconColor: "text-[#ff5511]",
    badgeBg: "bg-orange-500/20 text-orange-300 border-2 border-orange-500/40",
  },
  {
    channel: AudioChannel.CHAT,
    title: "Chat",
    sub: "routune-chat",
    icon: MessageSquare,
    iconBg: "bg-sky-500/15",
    iconBorder: "border-sky-500/30",
    iconColor: "text-sky-400",
    badgeBg: "bg-sky-500/20 text-sky-300 border-2 border-sky-500/40",
  },
  {
    channel: AudioChannel.MEDIA,
    title: "Media",
    sub: "routune-media",
    icon: Music,
    iconBg: "bg-purple-500/15",
    iconBorder: "border-purple-500/30",
    iconColor: "text-purple-400",
    badgeBg: "bg-purple-500/20 text-purple-300 border-2 border-purple-500/40",
  },
  {
    channel: AudioChannel.AUX,
    title: "Aux",
    sub: "routune-aux",
    icon: Radio,
    iconBg: "bg-emerald-500/15",
    iconBorder: "border-emerald-500/30",
    iconColor: "text-emerald-400",
    badgeBg: "bg-zinc-800 text-zinc-300 border-2 border-white/10",
  },
];

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

  return (
    <div
      ref={setNodeRef}
      style={style}
      {...attributes}
      {...listeners}
      className={`group flex items-center justify-between rounded-2xl border-2 px-3.5 py-3 select-none cursor-grab active:cursor-grabbing transition-colors ${
        isOverlay
          ? "border-[#ff5511] bg-[#282832] ring-2 ring-[#ff5511]/40 scale-105 z-50 text-white shadow-xl shadow-black/60"
          : isDragging
          ? "opacity-25 border-dashed border-zinc-600 bg-[#16161b]"
          : "border-white/10 bg-[#16161b] hover:border-white/20 hover:bg-[#202028] text-zinc-200"
      }`}
    >
      <div className="flex items-center gap-3 min-w-0">
        <div className="flex h-9 w-9 flex-shrink-0 items-center justify-center rounded-xl overflow-hidden bg-[#22222a] border-2 border-white/10 group-hover:border-white/20 transition-colors p-1">
          <AppIcon name={app.name} iconName={app.iconName} className="h-6 w-6" />
        </div>

        <div className="flex flex-col min-w-0">
          <span className="text-xs font-semibold text-white truncate leading-tight">
            {app.name || "Unknown App"}
          </span>
          <span className="text-[10px] font-mono text-zinc-400 truncate mt-0.5">
            node #{app.appId}
          </span>
        </div>
      </div>

      <div className="flex items-center gap-2 pl-2">
        {isRoutingApp ? (
          <Loader2 className="h-3.5 w-3.5 flex-shrink-0 animate-spin text-[#ff5511]" />
        ) : (
          <GripVertical className="h-3.5 w-3.5 flex-shrink-0 text-zinc-500 group-hover:text-zinc-300 transition-colors" />
        )}
      </div>
    </div>
  );
}

interface ChannelDropZoneProps {
  config: ChannelConfig;
  apps: AppStream[];
  currentRouting: string;
  onSelectRouting: (deviceName: string) => void;
  isUpdatingRouting?: boolean;
}

function ChannelDropZone({
  config,
  apps,
  currentRouting,
  onSelectRouting,
  isUpdatingRouting,
}: ChannelDropZoneProps) {
  const { isOver, setNodeRef } = useDroppable({
    id: `zone-${config.channel}`,
    data: { channel: config.channel },
  });

  const hardwareOutputs = useAudioStore((s) => s.hardwareOutputs);
  const Icon = config.icon;

  return (
    <div
      ref={setNodeRef}
      className={`flex flex-col justify-between rounded-3xl border-2 bg-[#202026] p-4 min-h-[300px] transition-colors ${
        isOver
          ? "border-[#ff5511]/70 bg-[#252530] ring-2 ring-[#ff5511]/30 scale-[1.01]"
          : "border-white/10 hover:border-white/20"
      }`}
    >
      {/* Card Header */}
      <div className="flex items-center justify-between pb-1">
        <div className="flex items-center gap-2.5">
          <div
            className={`flex h-9 w-9 items-center justify-center rounded-xl ${config.iconBg} border-2 ${config.iconBorder} ${config.iconColor}`}
          >
            <Icon className="h-4 w-4" />
          </div>
          <div>
            <h3 className="text-sm font-bold tracking-tight text-white leading-none">
              {config.title}
            </h3>
            <span className="text-[10px] font-mono text-zinc-400">{config.sub}</span>
          </div>
        </div>

        <div
          className={`flex h-5 w-5 items-center justify-center rounded-full text-[10px] font-mono font-medium ${
            apps.length > 0 ? config.badgeBg : "bg-zinc-800 text-zinc-400 border-2 border-white/10"
          }`}
        >
          {apps.length}
        </div>
      </div>

      {/* Droppable Body */}
      <div className="flex-1 my-3 flex flex-col gap-2 min-h-[140px] justify-start">
        {apps.length === 0 ? (
          <div
            className={`flex h-full min-h-[130px] flex-col items-center justify-center rounded-2xl border-2 border-dashed text-center p-4 transition-colors ${
              isOver
                ? "border-[#ff5511]/60 bg-[#ff5511]/10 text-white"
                : "border-white/10 bg-[#16161b] text-zinc-400"
            }`}
          >
            <ArrowDown className="h-4 w-4 mb-1 text-zinc-400" />
            <p className="text-xs font-medium text-zinc-300">Drop an app here</p>
          </div>
        ) : (
          apps.map((app) => <DraggableAppCard key={app.appId} app={app} />)
        )}
      </div>

      {/* Hardware Output Selector */}
      <div className="relative pt-1">
        <div className="relative flex items-center">
          <select
            value={currentRouting}
            disabled={isUpdatingRouting}
            onChange={(e) => onSelectRouting(e.target.value)}
            className="w-full appearance-none rounded-xl border-2 border-white/10 bg-[#16161b] py-2.5 pl-3 pr-8 text-xs font-medium text-zinc-200 focus:border-[#ff5511] focus:outline-none cursor-pointer disabled:opacity-50"
          >
            <option value="none">No output (muted)</option>
            {hardwareOutputs.map((d) => (
              <option key={d.id} value={d.id}>
                {d.displayName}
              </option>
            ))}
          </select>
          <div className="pointer-events-none absolute right-3 flex items-center text-zinc-400">
            {isUpdatingRouting ? (
              <Loader2 className="h-3 w-3 animate-spin text-[#ff5511]" />
            ) : (
              <ChevronDown className="h-3.5 w-3.5" />
            )}
          </div>
        </div>
      </div>
    </div>
  );
}

export function AppRoutingPanel() {
  const connectionStatus = useAudioStore((s) => s.connectionStatus);
  const activeApps = useAudioStore((s) => s.activeApps);
  const fetchActiveApps = useAudioStore((s) => s.fetchActiveApps);
  const hardwareInputs = useAudioStore((s) => s.hardwareInputs);
  const fetchHardwareInputs = useAudioStore((s) => s.fetchHardwareInputs);
  const routeApplication = useAudioStore((s) => s.routeApplication);
  const channelRoutings = useAudioStore((s) => s.channelRoutings);
  const setChannelRouting = useAudioStore((s) => s.setChannelRouting);
  const isRoutingUpdating = useAudioStore((s) => s.isRoutingUpdating);
  const micTestEnabled = useAudioStore((s) => s.micTestEnabled);
  const isTogglingMicTest = useAudioStore((s) => s.isTogglingMicTest);
  const toggleMicTest = useAudioStore((s) => s.toggleMicTest);
  const fetchMicTest = useAudioStore((s) => s.fetchMicTest);

  const [activeDragApp, setActiveDragApp] = useState<AppStream | null>(null);
  const [isManualRefreshing, setIsManualRefreshing] = useState(false);

  const sensors = useSensors(
    useSensor(PointerSensor, {
      activationConstraint: {
        distance: 5,
      },
    })
  );

  useEffect(() => {
    if (connectionStatus !== "connected") return;
    fetchActiveApps();
    fetchHardwareInputs();
    fetchMicTest();
    const interval = setInterval(() => {
      fetchActiveApps();
    }, 2500);
    return () => clearInterval(interval);
  }, [connectionStatus, fetchActiveApps, fetchHardwareInputs, fetchMicTest]);

  const handleManualRefresh = async () => {
    setIsManualRefreshing(true);
    await Promise.all([fetchActiveApps(), fetchHardwareInputs(), fetchMicTest()]);
    setTimeout(() => setIsManualRefreshing(false), 500);
  };

  const handleDragStart = (event: DragStartEvent) => {
    const app = event.active.data.current?.app as AppStream | undefined;
    if (app) setActiveDragApp(app);
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

  // Group apps by channel
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
        map[AudioChannel.MASTER].push(app);
      }
    });

    return map;
  }, [activeApps]);

  const unassignedApps = appsByChannel[AudioChannel.MASTER] || [];

  // Drop target for Unassigned container
  const { isOver: isOverUnassigned, setNodeRef: setUnassignedRef } = useDroppable({
    id: `zone-${AudioChannel.MASTER}`,
    data: { channel: AudioChannel.MASTER },
  });

  return (
    <div className="flex flex-col gap-6 w-full select-none">
      {/* View Title */}
      <div className="flex flex-col gap-1">
        <h1 className="text-xl font-bold tracking-tight text-white font-sans">
          Routing
        </h1>
      </div>

      {/* Mic Input Card (Physical Capture Hardware -> PipeWire Virtual Source) */}
      <div className="rounded-3xl border-2 border-white/10 bg-[#202026] p-5 flex flex-col md:flex-row md:items-center justify-between gap-4">
        <div className="flex items-center gap-3.5">
          <div className="flex h-11 w-11 shrink-0 items-center justify-center rounded-2xl bg-rose-500/15 border-2 border-rose-500/30 text-rose-400">
            <Mic className="h-5 w-5" />
          </div>
          <div className="flex flex-col gap-0.5">
            <div className="flex items-center gap-2">
              <h2 className="text-sm font-bold text-white tracking-tight">Mic Input</h2>
              <span className="rounded-md border border-rose-500/30 bg-rose-500/10 px-2 py-0.5 text-[10px] font-mono text-rose-300">
                Audio/Source/Virtual
              </span>
            </div>
            <p className="text-xs text-zinc-400">
              Direct physical microphone capture stream into "Routune Mic" PipeWire virtual source.
            </p>
          </div>
        </div>

        <div className="flex items-center gap-2.5 w-full md:w-auto">
          <button
            type="button"
            disabled={isTogglingMicTest}
            onClick={() => toggleMicTest()}
            className={`flex items-center gap-2 rounded-xl border-2 px-3.5 py-2.5 text-xs font-semibold transition-all shrink-0 ${
              micTestEnabled
                ? "border-emerald-500/60 bg-emerald-500/20 text-emerald-300 shadow-lg shadow-emerald-950/40"
                : "border-white/10 bg-[#16161b] text-zinc-400 hover:border-white/20 hover:text-zinc-200"
            } disabled:opacity-50`}
            title="Listen to microphone with DSP effects in real time through primary output"
          >
            {isTogglingMicTest ? (
              <Loader2 className="h-3.5 w-3.5 animate-spin text-emerald-400" />
            ) : (
              <Headphones className={`h-3.5 w-3.5 ${micTestEnabled ? "text-emerald-400" : "text-zinc-400"}`} />
            )}
            <span>{micTestEnabled ? "Testing Mic (Active)" : "Test Mic"}</span>
          </button>

          <div className="relative w-full md:w-72">
            <select
              value={channelRoutings[AudioChannel.MIC] || "none"}
              disabled={isRoutingUpdating[AudioChannel.MIC]}
              onChange={(e) => setChannelRouting(AudioChannel.MIC, e.target.value)}
              className="w-full appearance-none rounded-xl border-2 border-white/10 bg-[#16161b] py-2.5 pl-3 pr-8 text-xs font-medium text-zinc-200 focus:border-[#ff5511] focus:outline-none cursor-pointer disabled:opacity-50"
            >
              <option value="none">No input (disabled)</option>
              {hardwareInputs.map((d) => (
                <option key={d.id} value={d.id}>
                  {d.displayName}
                </option>
              ))}
            </select>
            <div className="pointer-events-none absolute right-3 top-1/2 -translate-y-1/2 flex items-center text-zinc-400">
              {isRoutingUpdating[AudioChannel.MIC] ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin text-[#ff5511]" />
              ) : (
                <ChevronDown className="h-3.5 w-3.5" />
              )}
            </div>
          </div>
        </div>
      </div>

      <DndContext
        sensors={sensors}
        onDragStart={handleDragStart}
        onDragEnd={handleDragEnd}
        onDragCancel={() => setActiveDragApp(null)}
      >
        {/* 4 Channel Columns Grid */}
        <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4">
          {CHANNEL_CONFIGS.map((config) => (
            <ChannelDropZone
              key={config.channel}
              config={config}
              apps={appsByChannel[config.channel] || []}
              currentRouting={channelRoutings[config.channel] || "none"}
              onSelectRouting={(dev) => setChannelRouting(config.channel, dev)}
              isUpdatingRouting={isRoutingUpdating[config.channel]}
            />
          ))}
        </div>

        {/* Bottom Container: Unassigned streams */}
        <div
          ref={setUnassignedRef}
          className={`rounded-3xl border-2 bg-[#202026] p-5 flex flex-col gap-4 transition-colors ${
            isOverUnassigned
              ? "border-[#ff5511]/70 bg-[#252530] ring-2 ring-[#ff5511]/30"
              : "border-white/10"
          }`}
        >
          {/* Header Row */}
          <div className="flex items-center justify-between">
            <div className="flex flex-col">
              <h2 className="text-sm font-bold text-white tracking-tight">
                Unassigned streams
              </h2>
            </div>

            <button
              type="button"
              onClick={handleManualRefresh}
              disabled={isManualRefreshing}
              className="flex items-center gap-2 rounded-xl border-2 border-white/10 bg-[#282830] hover:bg-[#32323c] hover:border-white/20 px-3.5 py-2 text-xs font-medium text-zinc-200 hover:text-white transition-colors cursor-pointer disabled:opacity-50"
            >
              <RotateCw className={`h-3.5 w-3.5 ${isManualRefreshing ? "animate-spin text-[#ff5511]" : ""}`} />
              <span>Scan apps</span>
            </button>
          </div>

          {/* Unassigned App Cards Row */}
          <div className="flex flex-wrap gap-3 min-h-[64px] items-center">
            {unassignedApps.length === 0 ? (
              <span className="text-xs font-mono text-zinc-400 py-3">
                No unassigned streams found. All active applications routed.
              </span>
            ) : (
              unassignedApps.map((app) => (
                <div key={app.appId} className="w-full sm:w-auto min-w-[220px]">
                  <DraggableAppCard app={app} />
                </div>
              ))
            )}
          </div>
        </div>

        {/* Drag Overlay during Dragging */}
        <DragOverlay dropAnimation={null}>
          {activeDragApp ? <DraggableAppCard app={activeDragApp} isOverlay /> : null}
        </DragOverlay>
      </DndContext>
    </div>
  );
}
