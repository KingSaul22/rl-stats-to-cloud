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
    api_key: z.string().optional(),
    apiKey: z.string().optional(),
    email: z.string().optional(),
    password: z.string().optional(),
  })
  .loose();

export const RawAppConfigSchema = z
  .object({
    connector: RawConnectorConfigSchema.optional(),
    reconnect_delay_seconds: z.number().optional(),
    reconnectDelaySeconds: z.number().optional(),
    is_headless: z.boolean().optional(),
    isHeadless: z.boolean().optional(),
    websocket_url: z.string().optional(),
    websocketUrl: z.string().optional(),
    ui_sync_port: z.number().optional(),
    uiSyncPort: z.number().optional(),
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
    apiKey: string;
    email: string;
    password: string;
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
  let connectorApiKey = "";
  let connectorEmail = "";
  let connectorPassword = "";

  if (connectorRaw && typeof connectorRaw === "object") {
    const conn = connectorRaw as {
      type?: unknown;
      url?: unknown;
      api_key?: unknown;
      apiKey?: unknown;
      email?: unknown;
      password?: unknown;
    };
    connectorType = typeof conn.type === "string" ? conn.type : "";
    connectorUrl = typeof conn.url === "string" ? conn.url : "";
    const apiKeyRaw = conn.apiKey ?? conn.api_key;
    connectorApiKey = typeof apiKeyRaw === "string" ? apiKeyRaw : "";
    connectorEmail = typeof conn.email === "string" ? conn.email : "";
    connectorPassword = typeof conn.password === "string" ? conn.password : "";
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
    validated.isHeadless ?? validated.is_headless ?? false;
  const websocketUrl = 
    validated.websocketUrl ?? validated.websocket_url ?? "ws://localhost:1420";
  const uiSyncPort = 
    validated.uiSyncPort ?? validated.ui_sync_port ?? 54321;

  return {
    connector: {
      type: connectorType,
      url: connectorUrl,
      apiKey: connectorApiKey,
      email: connectorEmail,
      password: connectorPassword,
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
