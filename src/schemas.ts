import { z } from "zod";
import { CONSTANTS } from "./constants";

// ============================================================================
// ZOD SCHEMAS (VALIDATION ONLY, WITH PASSTHROUGH)
// ============================================================================

// Raw API response types (may have snake_case or camelCase fields)
export const RawConnectorConfigSchema = z
  .object({
    type: z.string().optional(),
    url: z.string().optional(),
    auth_token: z.string().nullable().optional(),
    authToken: z.string().nullable().optional(),
  })
  .loose();

export const RawAppConfigSchema = z
  .object({
    connector: RawConnectorConfigSchema.optional(),
    reconnect_delay_seconds: z.number().optional(),
    reconnectDelaySeconds: z.number().optional(),
    is_headless: z.boolean().optional(),
    websocket_url: z.string().optional(),
    ui_sync_port: z.number().optional(),
  })
  .loose();

export const RawStatusPayloadSchema = z
  .object({
    is_connected: z.boolean().optional(),
    isConnected: z.boolean().optional(),
    last_event: z.string().optional(),
    lastEvent: z.string().optional(),
  })
  .loose();

// ============================================================================
// NORMALIZED INTERNAL TYPES (ALWAYS CAMELCASE, STRICTLY TYPED)
// ============================================================================

export type AppConfig = {
  connector: {
    type: string;
    url: string;
    authToken: string | null;
  };
  reconnectDelaySeconds: number;
  isHeadless: boolean;
  websocketUrl: string;
  uiSyncPort: number;
};

export type StatusPayload = {
  isConnected: boolean;
  lastEvent: string;
};

// ============================================================================
// EXPLICIT NORMALIZATION FUNCTIONS
// ============================================================================

/**
 * Normalize raw config from API to internal AppConfig.
 * Fails loudly if validation fails (no silent fallbacks).
 */
export function normalizeAppConfig(raw: unknown): AppConfig {
  const validated = RawAppConfigSchema.parse(raw);

  // Extract connector fields with explicit fallbacks
  const connectorRaw = validated.connector;
  let connectorType = "";
  let connectorUrl = "";
  let connectorAuthToken: string | null = null;

  if (connectorRaw && typeof connectorRaw === "object") {
    const conn = connectorRaw as { type?: unknown; url?: unknown; auth_token?: unknown; authToken?: unknown };
    connectorType = typeof conn.type === "string" ? conn.type : "";
    connectorUrl = typeof conn.url === "string" ? conn.url : "";
    const authRaw = conn.auth_token ?? conn.authToken;
    connectorAuthToken = typeof authRaw === "string" ? authRaw : null;
  }

  const reconnectDelay =
    typeof validated.reconnect_delay_seconds === "number"
      ? validated.reconnect_delay_seconds
      : typeof validated.reconnectDelaySeconds === "number"
        ? validated.reconnectDelaySeconds
        : undefined;

  if (reconnectDelay !== undefined && (!Number.isFinite(reconnectDelay) || reconnectDelay <= 0)) {
    throw new Error(`Invalid reconnect delay: ${reconnectDelay}. Must be a positive number.`);
  }

  const isHeadless =
    typeof validated.is_headless === "boolean" ? validated.is_headless : false;
  const websocketUrl =
    typeof validated.websocket_url === "string" ? validated.websocket_url : "ws://localhost:1420";
  const uiSyncPort =
    typeof validated.ui_sync_port === "number" ? validated.ui_sync_port : 54321;

  return {
    connector: {
      type: connectorType,
      url: connectorUrl,
      authToken: connectorAuthToken,
    },
    reconnectDelaySeconds: reconnectDelay ?? CONSTANTS.DEFAULT_DELAYS.RECONNECT,
    isHeadless,
    websocketUrl,
    uiSyncPort,
  };
}

/**
 * Normalize raw status payload from API to internal StatusPayload.
 * Fails loudly if validation fails (no silent fallbacks).
 */
export function normalizeStatusPayload(raw: unknown): StatusPayload {
  const validated = RawStatusPayloadSchema.parse(raw);

  const isConnected = validated.is_connected ?? validated.isConnected ?? false;
  const lastEvent = validated.last_event ?? validated.lastEvent ?? "";

  if (typeof isConnected !== "boolean") {
    throw new Error(`Invalid isConnected: expected boolean, got ${typeof isConnected}`);
  }
  if (typeof lastEvent !== "string") {
    throw new Error(`Invalid lastEvent: expected string, got ${typeof lastEvent}`);
  }

  return {
    isConnected,
    lastEvent,
  };
}

// ============================================================================
// RUNTIME VALIDATION & DATA BOUNDARIES
// ============================================================================

/**
 * Parse and validate app config from raw API response.
 * Throws on validation failure for explicit error handling.
 */
export function parseAppConfig(data: unknown): AppConfig {
  return normalizeAppConfig(data);
}

/**
 * Parse and validate status payload from raw API response.
 * Throws on validation failure for explicit error handling.
 */
export function parseStatusPayload(data: unknown): StatusPayload {
  return normalizeStatusPayload(data);
}
