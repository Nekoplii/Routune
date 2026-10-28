import { GrpcWebFetchTransport } from "@protobuf-ts/grpcweb-transport";
import { RoutuneControlClient } from "../proto/routune.client";

const DEFAULT_DAEMON_URL = "http://127.0.0.1:50051";

export const transport = new GrpcWebFetchTransport({
  baseUrl: (import.meta as unknown as { env?: { VITE_DAEMON_URL?: string } }).env?.VITE_DAEMON_URL || DEFAULT_DAEMON_URL,
});

export const routuneClient = new RoutuneControlClient(transport);
