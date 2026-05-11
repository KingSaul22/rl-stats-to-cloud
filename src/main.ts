import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

type JsonRecord = Record<string, unknown>;

type ConnectorConfig = {
  type?: unknown;
  url?: unknown;
  auth_token?: unknown;
  authToken?: unknown;
};

type AppConfig = JsonRecord & {
  connector?: unknown;
  reconnect_delay_seconds?: unknown;
  reconnectDelaySeconds?: unknown;
};

type StatusPayload = {
  is_connected?: unknown;
  isConnected?: unknown;
  last_event?: unknown;
  lastEvent?: unknown;
};

let currentConfig: AppConfig = {};
let saveFeedbackTimer: number | undefined;

const connectionStatusEl = document.querySelector<HTMLElement>("#connection-status");
const lastEventEl = document.querySelector<HTMLElement>("#last-event");
const formEl = document.querySelector<HTMLFormElement>("#config-form");
const saveButtonEl = document.querySelector<HTMLButtonElement>("#save-config");
const saveStatusEl = document.querySelector<HTMLElement>("#save-status");
const connectorTypeEl = document.querySelector<HTMLSelectElement>("#connector-type");
const connectorUrlEl = document.querySelector<HTMLInputElement>("#connector-url");
const connectorAuthTokenEl = document.querySelector<HTMLInputElement>("#connector-auth-token");
const reconnectDelayEl = document.querySelector<HTMLInputElement>("#reconnect-delay-seconds");

function readConfigField(
  config: JsonRecord,
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
  if (!connectionStatusEl) {
    return;
  }

  connectionStatusEl.classList.remove("connected", "disconnected", "waiting");

  if (!isInitialized) {
    connectionStatusEl.textContent = "Waiting";
    connectionStatusEl.classList.add("waiting");
    return;
  }

  connectionStatusEl.textContent = isConnected ? "Connected" : "Disconnected";
  connectionStatusEl.classList.add(isConnected ? "connected" : "disconnected");
}

function setLastEvent(value: unknown): void {
  if (!lastEventEl) {
    return;
  }

  lastEventEl.textContent = normalizeEventLabel(value);
}

function applyStatus(status: StatusPayload | null | undefined): void {
  const isConnected = Boolean(status?.is_connected ?? status?.isConnected);
  const lastEvent = status?.last_event ?? status?.lastEvent ?? "";

  setConnectionStatus(isConnected, true);
  setLastEvent(lastEvent);
}

function showSaveMessage(message: string, isSuccess = true): void {
  if (!saveStatusEl) {
    return;
  }

  if (saveFeedbackTimer !== undefined) {
    window.clearTimeout(saveFeedbackTimer);
  }

  saveStatusEl.textContent = message;
  saveStatusEl.style.color = isSuccess ? "#59d185" : "#ef6461";

  saveFeedbackTimer = window.setTimeout(() => {
    if (saveStatusEl) {
      saveStatusEl.textContent = "";
    }
  }, 2200);
}

async function loadInitialConfig(): Promise<void> {
  const config = await invoke<unknown>("get_config");
  currentConfig =
    typeof config === "object" && config !== null ? (config as AppConfig) : {};

  const connectorRaw = currentConfig.connector;
  const connector: JsonRecord =
    typeof connectorRaw === "object" && connectorRaw !== null
      ? (connectorRaw as JsonRecord)
      : {};

  if (connectorTypeEl) {
    connectorTypeEl.value = String(readConfigField(connector, "type", "type") || "Firebase");
  }

  if (connectorUrlEl) {
    connectorUrlEl.value = String(readConfigField(connector, "url", "url"));
  }

  if (connectorAuthTokenEl) {
    connectorAuthTokenEl.value = String(
      readConfigField(connector, "auth_token", "authToken")
    );
  }

  if (reconnectDelayEl) {
    const reconnectDelay = Number(
      readConfigField(
        currentConfig,
        "reconnect_delay_seconds",
        "reconnectDelaySeconds"
      )
    );
    reconnectDelayEl.value = String(
      Number.isFinite(reconnectDelay) && reconnectDelay > 0 ? reconnectDelay : 5
    );
  }
}

async function loadInitialStatus(): Promise<void> {
  setConnectionStatus(false, false);
  setLastEvent("");

  const status = await invoke<StatusPayload>("get_status");
  applyStatus(status);
}

async function saveConfiguration(event: Event): Promise<void> {
  event.preventDefault();

  if (
    !connectorTypeEl ||
    !connectorUrlEl ||
    !connectorAuthTokenEl ||
    !reconnectDelayEl ||
    !saveButtonEl
  ) {
    return;
  }

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
    await invoke("save_config", { newConfig });
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
  if (formEl) {
    formEl.addEventListener("submit", saveConfiguration);
  }

  void initialize();
});
