import { useEffect } from "react";
import { routuneClient } from "../services/grpcClient";
import { useAudioStore } from "../store/useAudioStore";
import type { ChannelVolumeMetric } from "../proto/routune";

export function useTelemetryStream() {
  const setConnectionStatus = useAudioStore((s) => s.setConnectionStatus);
  const setAudioActive = useAudioStore((s) => s.setAudioActive);
  const setLatencyMs = useAudioStore((s) => s.setLatencyMs);
  const updateTelemetry = useAudioStore((s) => s.updateTelemetry);

  useEffect(() => {
    let isCancelled = false;
    let abortController: AbortController | null = null;
    let reconnectTimer: ReturnType<typeof setTimeout> | null = null;

    async function connectAndStream() {
      if (isCancelled) return;

      abortController = new AbortController();
      setConnectionStatus("connecting");

      try {
        // 1. Initial health ping
        const pingStart = performance.now();
        const pingCall = routuneClient.ping(
          { clientId: "routune-gui" },
          { abort: abortController.signal }
        );
        const pingRes = await pingCall.response;

        if (isCancelled) return;

        const roundtrip = Math.round(performance.now() - pingStart);
        setLatencyMs(roundtrip);
        setAudioActive(pingRes.audioActive);
        setConnectionStatus("connected");

        // Fetch physical hardware output devices and authoritative daemon config
        useAudioStore.getState().fetchHardwareOutputs();
        useAudioStore.getState().fetchHardwareInputs();
        useAudioStore.getState().fetchDaemonConfig();
        useAudioStore.getState().fetchActiveApps();

        // 2. Consume server-streaming telemetry with requestAnimationFrame throttling
        const streamCall = routuneClient.streamTelemetry(
          { targetFps: 60 },
          { abort: abortController.signal }
        );

        let pendingChunk: {
          peakDb: number;
          rmsDb: number;
          seq: string;
          metrics?: ChannelVolumeMetric[];
        } | null = null;
        let rafId: number | null = null;

        const scheduleFlush = () => {
          if (rafId !== null) return;
          rafId = requestAnimationFrame(() => {
            rafId = null;
            if (pendingChunk && !isCancelled) {
              updateTelemetry(
                pendingChunk.peakDb,
                pendingChunk.rmsDb,
                pendingChunk.seq,
                pendingChunk.metrics
              );
              pendingChunk = null;
            }
          });
        };

        for await (const chunk of streamCall.responses) {
          if (isCancelled) break;
          pendingChunk = {
            peakDb: chunk.peakVolumeDb,
            rmsDb: chunk.rmsVolumeDb,
            seq: chunk.sequenceNumber.toString(),
            metrics: chunk.channelMetrics,
          };
          scheduleFlush();
        }

        if (rafId !== null) {
          cancelAnimationFrame(rafId);
          rafId = null;
        }

        // Stream completed gracefully
        if (!isCancelled) {
          setConnectionStatus("disconnected");
          scheduleReconnect();
        }
      } catch (err: unknown) {
        if (isCancelled) return;
        const msg = err instanceof Error ? err.message : String(err);
        setConnectionStatus("error", msg);
        scheduleReconnect();
      }
    }

    function scheduleReconnect() {
      if (isCancelled) return;
      reconnectTimer = setTimeout(() => {
        connectAndStream();
      }, 1500);
    }

    connectAndStream();

    return () => {
      isCancelled = true;
      if (abortController) {
        abortController.abort();
      }
      if (reconnectTimer) {
        clearTimeout(reconnectTimer);
      }
    };
  }, [setConnectionStatus, setAudioActive, setLatencyMs, updateTelemetry]);
}
