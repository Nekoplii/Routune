import { useEffect } from "react";
import { routuneClient } from "../services/grpcClient";
import { useAudioStore } from "../store/useAudioStore";

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

        // Fetch physical hardware output devices for Sonar routing dropdowns
        useAudioStore.getState().fetchHardwareOutputs();

        // 2. Consume server-streaming telemetry
        const streamCall = routuneClient.streamTelemetry(
          { targetFps: 60 },
          { abort: abortController.signal }
        );

        for await (const chunk of streamCall.responses) {
          if (isCancelled) break;
          updateTelemetry(
            chunk.peakVolumeDb,
            chunk.rmsVolumeDb,
            chunk.sequenceNumber.toString(),
            chunk.channelMetrics
          );
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
