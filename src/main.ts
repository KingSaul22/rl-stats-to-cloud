import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

// 1. STRIcT TYPES
type ConnectorConfig = {
  type?: string;
  url?: string;
  auth_token?: string | null;
  authToken?: string | null;
};

type AppConfig = Record<string, unknown> & {
  connector?: ConnectorConfig;
  reconnect_delay_seconds?: number;
  reconnectDelaySeconds?: number;
};

type StatusPayload = {
  is_connected?: boolean;
  isConnected?: boolean;
  last_event?: string;
  lastEvent?: string;
};

let currentConfig: AppConfig = {};
let saveFeedbackTimer: ReturnType<typeof setTimeout> | undefined;

// 2. NON-NULL ASSERTIONS (!)
// Assuming these elements are hardcoded in your HTML and always exist.
const connectionStatusEl = document.querySelector<HTMLElement>("#connection-status")!;
const lastEventEl = document.querySelector<HTMLElement>("#last-event")!;
const formEl = document.querySelector<HTMLFormElement>("#config-form")!;
const saveButtonEl = document.querySelector<HTMLButtonElement>("#save-config")!;
const saveStatusEl = document.querySelector<HTMLElement>("#save-status")!;
const connectorTypeEl = document.querySelector<HTMLSelectElement>("#connector-type")!;
const connectorUrlEl = document.querySelector<HTMLInputElement>("#connector-url")!;
const connectorAuthTokenEl = document.querySelector<HTMLInputElement>("#connector-auth-token")!;
const reconnectDelayEl = document.querySelector<HTMLInputElement>("#reconnect-delay-seconds")!;

function readConfigField(
  config: Record<string, unknown>,
  snakeCase: string,
  camelCase = ""
): unknown {
  if (Object.prototype.hasOwnProperty.call(config, snakeCase)) {
    return config[snakeCase];
  }

  if (camelCase && Object.prototype.hasOwnProperty.call(config, camelCase)) {
    return config[camelCase];
  }

  return "";
}

function normalizeEventLabel(rawEvent: unknown): string {
  const text = typeof rawEvent === "string" ? rawEvent.trim() : "";
  return text || "None";
}

function setConnectionStatus(isConnected: boolean, isInitialized = true): void {
  if (!isInitialized) {
    connectionStatusEl.textContent = "Connecting...";
    connectionStatusEl.className = "status-connecting";
    return;
  }

  if (isConnected) {
    connectionStatusEl.textContent = "Connected";
    connectionStatusEl.className = "status-connected";
  } else {
    connectionStatusEl.textContent = "Disconnected";
    connectionStatusEl.className = "status-disconnected";
  }
}

function applyStatus(status: StatusPayload): void {
  const isConnected = status.is_connected ?? status.isConnected ?? false;
  setConnectionStatus(Boolean(isConnected));

  const lastEvent = status.last_event ?? status.lastEvent;
  lastEventEl.textContent = normalizeEventLabel(lastEvent);
}

function applyConfigToForm(config: AppConfig): void {
  const connector = config.connector || {};
  
  connectorTypeEl.value = (connector.type as string) || "Firebase";
  connectorUrlEl.value = (connector.url as string) || "";
  connectorAuthTokenEl.value = (connector.auth_token as string) || (connector.authToken as string) || "";

  const delay = readConfigField(config, "reconnect_delay_seconds", "reconnectDelaySeconds");
  reconnectDelayEl.value = delay !== "" ? String(delay) : "5";
}

async function loadInitialConfig(): Promise<void> {
  try {
    const config = await invoke<AppConfig>("get_config");
    currentConfig = config || {};
    applyConfigToForm(currentConfig);
  } catch (error) {
    console.error("Failed to load initial config", error);
  }
}

async function loadInitialStatus(): Promise<void> {
  setConnectionStatus(false, false);
  try {
    const status = await invoke<StatusPayload>("get_status");
    if (status) {
      applyStatus(status);
    } else {
      setConnectionStatus(false);
    }
  } catch (error) {
    console.error("Failed to load initial status", error);
    setConnectionStatus(false);
  }
}

function showSaveMessage(message: string, isSuccess: boolean): void {
  saveStatusEl.textContent = message;
  saveStatusEl.className = isSuccess ? "status-connected" : "status-disconnected";
  saveStatusEl.style.opacity = "1";

  if (saveFeedbackTimer) {
    clearTimeout(saveFeedbackTimer);
  }

  saveFeedbackTimer = setTimeout(() => {
    saveStatusEl.style.opacity = "0";
  }, 3000);
}

async function saveConfig(e: Event): Promise<void> {
  e.preventDefault();

  const previousButtonLabel = saveButtonEl.textContent;
  saveButtonEl.disabled = true;
  saveButtonEl.textContent = "Saving...";

  const reconnectDelay = Number(reconnectDelayEl.value);
  const safeReconnectDelay =
    Number.isFinite(reconnectDelay) && reconnectDelay > 0
      ? Math.floor(reconnectDelay)
      : 5;

  const connector: ConnectorConfig = {
    type: connectorTypeEl.value || "Firebase",
    url: connectorUrlEl.value.trim(),
    auth_token: connectorAuthTokenEl.value || null,
  };

  const newConfig: AppConfig = {
    ...currentConfig,
    connector,
    reconnect_delay_seconds: safeReconnectDelay,
  };

  try {
    // Note: passing the expected return type to invoke helps TS infer the response
    await invoke<void>("save_config", { newConfig });
    currentConfig = newConfig;
    reconnectDelayEl.value = String(safeReconnectDelay);
    showSaveMessage("Saved successfully!", true);
  } catch (error) {
    console.error("Failed to save configuration", error);
    showSaveMessage("Failed to save configuration", false);
  } finally {
    saveButtonEl.disabled = false;
    saveButtonEl.textContent = previousButtonLabel || "Save Configuration";
  }
}

async function initialize(): Promise<void> {
  try {
    await loadInitialConfig();
    await loadInitialStatus();

    // Tauri's listen event payload can be typed!
    await listen<StatusPayload>("status-update", (event) => {
      if (event?.payload && typeof event.payload === "object") {
        applyStatus(event.payload);
      }
    });
  } catch (error) {
    console.error("Failed to initialize frontend", error);
    showSaveMessage("Failed to initialize app state", false);
  }
}

window.addEventListener("DOMContentLoaded", () => {
  formEl.addEventListener("submit", saveConfig);
  initialize();
});