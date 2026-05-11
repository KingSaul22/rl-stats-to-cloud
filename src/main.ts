import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { z } from "zod";

// ============================================================================
// PHASE 1: COMMANDS & CONSTANTS
// ============================================================================

const COMMANDS = {
  GET_CONFIG: "get_config",
  SAVE_CONFIG: "save_config",
  GET_STATUS: "get_status",
} as const;

const CONSTANTS = {
  CONNECTOR_TYPES: {
    FIREBASE: "Firebase",
  },
  CONNECTION_STATES: {
    CONNECTING: "connecting",
    CONNECTED: "connected",
    DISCONNECTED: "disconnected",
  },
  UI_MESSAGES: {
    SAVING: "Saving...",
    SAVED_SUCCESS: "Saved successfully!",
    SAVED_ERROR: "Failed to save configuration",
    INIT_ERROR: "Failed to initialize app state",
    CONNECTING: "Connecting...",
    CONNECTED: "Connected",
    DISCONNECTED: "Disconnected",
    NONE_EVENT: "None",
  },
  DEFAULT_DELAYS: {
    RECONNECT: 5,
    MESSAGE_FADE: 3000,
  },
  UI_SELECTORS: {
    CONNECTION_STATUS: "#connection-status",
    LAST_EVENT: "#last-event",
    CONFIG_FORM: "#config-form",
    SAVE_BUTTON: "#save-config",
    SAVE_STATUS: "#save-status",
    CONNECTOR_TYPE: "#connector-type",
    CONNECTOR_URL: "#connector-url",
    CONNECTOR_AUTH_TOKEN: "#connector-auth-token",
    RECONNECT_DELAY: "#reconnect-delay-seconds",
  },
  CSS_CLASSES: {
    STATUS_CONNECTING: "status-connecting",
    STATUS_CONNECTED: "status-connected",
    STATUS_DISCONNECTED: "status-disconnected",
  },
} as const;

// ============================================================================
// ZOD SCHEMAS (VALIDATION ONLY, NO TRANSFORMS)
// ============================================================================

// Raw API response types (may have snake_case or camelCase fields)
const RawConnectorConfigSchema = z
  .object({
    type: z.string().optional(),
    url: z.string().optional(),
    auth_token: z.string().nullable().optional(),
    authToken: z.string().nullable().optional(),
  })
  .strict();

const RawAppConfigSchema = z
  .object({
    connector: RawConnectorConfigSchema.optional(),
    reconnect_delay_seconds: z.number().optional(),
    reconnectDelaySeconds: z.number().optional(),
    is_headless: z.boolean().catch(false),
    websocket_url: z.string().catch("ws://localhost:1420"),
    ui_sync_port: z.number().catch(54321),
  })
  .strict();

const RawStatusPayloadSchema = z
  .object({
    is_connected: z.boolean().optional(),
    isConnected: z.boolean().optional(),
    last_event: z.string().optional(),
    lastEvent: z.string().optional(),
  })
  .strict();

// Normalized internal types (always camelCase, strictly typed)
type AppConfig = {
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

type StatusPayload = {
  isConnected: boolean;
  lastEvent: string;
};

// ============================================================================
// PHASE 2: EXPLICIT NORMALIZATION FUNCTIONS
// ============================================================================

/**
 * Normalize raw config from API to internal AppConfig.
 * Fails loudly if validation fails (no silent fallbacks).
 */
function normalizeAppConfig(raw: unknown): AppConfig {
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
function normalizeStatusPayload(raw: unknown): StatusPayload {
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
// PHASE 3: RUNTIME VALIDATION & DATA BOUNDARIES
// ============================================================================

/**
 * Parse and validate app config from raw API response.
 * Throws on validation failure for explicit error handling.
 */
function parseAppConfig(data: unknown): AppConfig {
  return normalizeAppConfig(data);
}

/**
 * Parse and validate status payload from raw API response.
 * Throws on validation failure for explicit error handling.
 */
function parseStatusPayload(data: unknown): StatusPayload {
  return normalizeStatusPayload(data);
}

// ============================================================================
// PHASE 4: CENTRALIZED STATE & API LAYER
// ============================================================================

// Default config and status values
const DEFAULT_CONFIG: AppConfig = {
  connector: { type: "", url: "", authToken: null },
  reconnectDelaySeconds: CONSTANTS.DEFAULT_DELAYS.RECONNECT,
  isHeadless: false,
  websocketUrl: "ws://localhost:1420",
  uiSyncPort: 54321,
};

const DEFAULT_STATUS: StatusPayload = {
  isConnected: false,
  lastEvent: "",
};

// Centralized mutable state object
const state: { config: AppConfig; status: StatusPayload } = {
  config: { ...DEFAULT_CONFIG },
  status: { ...DEFAULT_STATUS },
};

// API layer with explicit command constants
const api = {
  async getConfig(): Promise<AppConfig> {
    const data = await invoke<unknown>(COMMANDS.GET_CONFIG);
    return parseAppConfig(data);
  },

  async saveConfig(config: AppConfig): Promise<void> {
    await invoke<void>(COMMANDS.SAVE_CONFIG, { newConfig: config });
  },

  async getStatus(): Promise<StatusPayload> {
    const data = await invoke<unknown>(COMMANDS.GET_STATUS);
    return parseStatusPayload(data);
  },
};

// ============================================================================
// PHASE 5: DOM SAFETY & UI RENDERING
// ============================================================================

function requiredElement<T extends Element>(selector: string, context = ""): T {
  const element = document.querySelector<T>(selector);
  if (!element) {
    throw new Error(`DOM element not found: ${selector}${context ? ` (${context})` : ""}`);
  }
  return element;
}

function renderConnectionStatus(isConnected: boolean): void {
  const el = requiredElement<HTMLElement>(CONSTANTS.UI_SELECTORS.CONNECTION_STATUS, "connection-status");

  // Remove all status classes
  el.classList.remove(
    CONSTANTS.CSS_CLASSES.STATUS_CONNECTING,
    CONSTANTS.CSS_CLASSES.STATUS_CONNECTED,
    CONSTANTS.CSS_CLASSES.STATUS_DISCONNECTED
  );

  if (isConnected) {
    el.textContent = CONSTANTS.UI_MESSAGES.CONNECTED;
    el.classList.add(CONSTANTS.CSS_CLASSES.STATUS_CONNECTED);
  } else {
    el.textContent = CONSTANTS.UI_MESSAGES.DISCONNECTED;
    el.classList.add(CONSTANTS.CSS_CLASSES.STATUS_DISCONNECTED);
  }
}

function renderConnectionStatusConnecting(): void {
  const el = requiredElement<HTMLElement>(CONSTANTS.UI_SELECTORS.CONNECTION_STATUS, "connection-status");

  el.classList.remove(
    CONSTANTS.CSS_CLASSES.STATUS_CONNECTING,
    CONSTANTS.CSS_CLASSES.STATUS_CONNECTED,
    CONSTANTS.CSS_CLASSES.STATUS_DISCONNECTED
  );

  el.textContent = CONSTANTS.UI_MESSAGES.CONNECTING;
  el.classList.add(CONSTANTS.CSS_CLASSES.STATUS_CONNECTING);
}

function renderLastEvent(lastEvent: string): void {
  const el = requiredElement<HTMLElement>(CONSTANTS.UI_SELECTORS.LAST_EVENT, "last-event");
  const normalized = lastEvent.trim() || CONSTANTS.UI_MESSAGES.NONE_EVENT;
  el.textContent = normalized;
}

function renderStatus(status: StatusPayload): void {
  renderConnectionStatus(status.isConnected);
  renderLastEvent(status.lastEvent);
}

function renderConfigForm(config: AppConfig): void {
  const typeEl = requiredElement<HTMLSelectElement>(CONSTANTS.UI_SELECTORS.CONNECTOR_TYPE, "connector-type");
  const urlEl = requiredElement<HTMLInputElement>(CONSTANTS.UI_SELECTORS.CONNECTOR_URL, "connector-url");
  const authEl = requiredElement<HTMLInputElement>(
    CONSTANTS.UI_SELECTORS.CONNECTOR_AUTH_TOKEN,
    "connector-auth-token"
  );
  const delayEl = requiredElement<HTMLInputElement>(CONSTANTS.UI_SELECTORS.RECONNECT_DELAY, "reconnect-delay");

  typeEl.value = config.connector.type || CONSTANTS.CONNECTOR_TYPES.FIREBASE;
  urlEl.value = config.connector.url;
  authEl.value = config.connector.authToken || "";
  delayEl.value = String(config.reconnectDelaySeconds);
}

function renderSaveMessage(message: string, isSuccess: boolean): void {
  const el = requiredElement<HTMLElement>(CONSTANTS.UI_SELECTORS.SAVE_STATUS, "save-status");
  el.textContent = message;
  el.style.color = isSuccess ? "#59d185" : "#ef6461";
}

// ============================================================================
// PHASE 6: LIFECYCLE, ASYNC & ERROR HANDLING
// ============================================================================

let saveFeedbackTimer: ReturnType<typeof setTimeout> | undefined;
let unlistenFn: (() => void) | undefined;

function logError(context: string, error: unknown): void {
  const errorMsg = error instanceof Error ? error.message : String(error);
  console.error(`[${context}] ${errorMsg}`);
}

function resetSaveMessageTimeout(): void {
  if (saveFeedbackTimer !== undefined) {
    clearTimeout(saveFeedbackTimer);
  }
}

function startSaveMessageTimeout(): void {
  resetSaveMessageTimeout();
  saveFeedbackTimer = setTimeout(() => {
    const el = requiredElement<HTMLElement>(CONSTANTS.UI_SELECTORS.SAVE_STATUS, "save-status");
    el.textContent = "";
  }, CONSTANTS.DEFAULT_DELAYS.MESSAGE_FADE);
}

async function loadConfig(): Promise<void> {
  const config = await api.getConfig();
  state.config = config;
  renderConfigForm(config);
}

async function loadStatus(): Promise<void> {
  const status = await api.getStatus();
  state.status = status;
  renderStatus(status);
}

async function handleSaveConfig(event: Event): Promise<void> {
  event.preventDefault();

  const saveBtn = requiredElement<HTMLButtonElement>(CONSTANTS.UI_SELECTORS.SAVE_BUTTON, "save-button");
  const typeEl = requiredElement<HTMLSelectElement>(CONSTANTS.UI_SELECTORS.CONNECTOR_TYPE, "connector-type");
  const urlEl = requiredElement<HTMLInputElement>(CONSTANTS.UI_SELECTORS.CONNECTOR_URL, "connector-url");
  const authEl = requiredElement<HTMLInputElement>(
    CONSTANTS.UI_SELECTORS.CONNECTOR_AUTH_TOKEN,
    "connector-auth-token"
  );
  const delayEl = requiredElement<HTMLInputElement>(CONSTANTS.UI_SELECTORS.RECONNECT_DELAY, "reconnect-delay");

  const previousButtonLabel = saveBtn.textContent;
  saveBtn.disabled = true;
  saveBtn.textContent = CONSTANTS.UI_MESSAGES.SAVING;

  const reconnectDelay = Number(delayEl.value);
  if (!Number.isFinite(reconnectDelay) || reconnectDelay <= 0) {
    logError("handleSaveConfig", `Invalid reconnect delay: ${reconnectDelay}`);
    renderSaveMessage(CONSTANTS.UI_MESSAGES.SAVED_ERROR, false);
    saveBtn.disabled = false;
    saveBtn.textContent = previousButtonLabel || "Save";
    return;
  }

  const newConfig: AppConfig = {
    connector: {
      type: typeEl.value || CONSTANTS.CONNECTOR_TYPES.FIREBASE,
      url: urlEl.value.trim(),
      authToken: authEl.value || null,
    },
    reconnectDelaySeconds: Math.floor(reconnectDelay),
    isHeadless: state.config.isHeadless, // Preserve existing headless setting
    websocketUrl: state.config.websocketUrl, // Preserve existing websocket URL
    uiSyncPort: state.config.uiSyncPort, // Preserve existing UI sync port
  };

  try {
    await api.saveConfig(newConfig);
    state.config = newConfig;
    delayEl.value = String(newConfig.reconnectDelaySeconds);
    renderSaveMessage(CONSTANTS.UI_MESSAGES.SAVED_SUCCESS, true);
  } catch (error) {
    logError("handleSaveConfig", error);
    renderSaveMessage(CONSTANTS.UI_MESSAGES.SAVED_ERROR, false);
  } finally {
    startSaveMessageTimeout();
    saveBtn.disabled = false;
    saveBtn.textContent = previousButtonLabel || CONSTANTS.UI_MESSAGES.SAVED_SUCCESS;
  }
}

async function registerStatusListener(): Promise<void> {
  unlistenFn = await listen<unknown>("status-update", (event) => {
    if (event?.payload && typeof event.payload === "object") {
      try {
        const status = parseStatusPayload(event.payload);
        state.status = status;
        renderStatus(status);
      } catch (error) {
        logError("registerStatusListener: parseStatusPayload", error);
      }
    }
  });
}

async function initialize(): Promise<void> {
  try {
    // Register listener FIRST to avoid race condition
    await registerStatusListener();

    // Show connecting state
    renderConnectionStatusConnecting();

    // Load initial state
    await loadConfig();
    await loadStatus();
  } catch (error) {
    logError("initialize", error);
    renderSaveMessage(CONSTANTS.UI_MESSAGES.INIT_ERROR, false);
  }
}

function cleanup(): void {
  resetSaveMessageTimeout();
  if (unlistenFn) {
    unlistenFn();
  }
}

// ============================================================================
// PHASE 7: FINAL ASSEMBLY & TESTING
// ============================================================================

window.addEventListener("DOMContentLoaded", () => {
  const formEl = requiredElement<HTMLFormElement>(CONSTANTS.UI_SELECTORS.CONFIG_FORM, "config-form");
  formEl.addEventListener("submit", handleSaveConfig);

  void initialize();
});

window.addEventListener("beforeunload", () => {
  cleanup();
});