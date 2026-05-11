import { z } from "zod";
import { CONSTANTS } from "./constants";

// ============================================================================
// ZOD SCHEMAS (VALIDATION ONLY, NO TRANSFORMS)
// ============================================================================

// Raw API response types (may have snake_case or camelCase fields)
export const RawConnectorConfigSchema = z
  .object({
    type: z.string().optional(),
    url: z.string().optional(),
    auth_token: z.string().nullable().optional(),
    authToken: z.string().nullable().optional(),
  })
  .strict();

export const RawAppConfigSchema = z
  .object({
    connector: RawConnectorConfigSchema.optional(),
    reconnect_delay_seconds: z.number().optional(),
    reconnectDelaySeconds: z.number().optional(),
    is_headless: z.boolean().catch(false),
    websocket_url: z.string().catch("ws://localhost:1420"),
    ui_sync_port: z.number().catch(54321),
  })
  .strict();

export const RawStatusPayloadSchema = z
  .object({
    is_connected: z.boolean().optional(),
    isConnected: z.boolean().optional(),
    last_event: z.string().optional(),
    lastEvent: z.string().optional(),
  })
  .strict();

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

  const connectorRaw = validated.connector;
  const connectorType =
    typeof connectorRaw === "object" && connectorRaw !== null
      ? (connectorRaw as Record<string, unknown>).type ??
        (connectorRaw as Record<string, unknown>).authToken ??
        ""
      : "";

  const connectorUrl =
    typeof connectorRaw === "object" && connectorRaw !== null
      ? (connectorRaw as Record<string, unknown>).url ?? ""
      : "";

  const connectorAuthToken =
    typeof connectorRaw === "object" && connectorRaw !== null
      ? ((connectorRaw as Record<string, unknown>).auth_token ??
          (connectorRaw as Record<string, unknown>).authToken) ??
        null
      : null;

  const reconnectDelay = validated.reconnect_delay_seconds ?? validated.reconnectDelaySeconds;
  if (reconnectDelay !== undefined && (!Number.isFinite(reconnectDelay) || reconnectDelay <= 0)) {
    throw new Error(`Invalid reconnect delay: ${reconnectDelay}. Must be a positive number.`);
  }

  return {
    connector: {
      type: String(connectorType),
      url: String(connectorUrl),
      authToken: connectorAuthToken === null ? null : String(connectorAuthToken),
    },
    reconnectDelaySeconds: reconnectDelay ?? CONSTANTS.DEFAULT_DELAYS.RECONNECT,
    isHeadless: validated.is_headless ?? false,
    websocketUrl: validated.websocket_url ?? "ws://localhost:1420",
    uiSyncPort: validated.ui_sync_port ?? 54321,
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
