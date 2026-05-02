import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

/** @type {Record<string, unknown>} */
let currentConfig = {};
let saveFeedbackTimer;

const connectionStatusEl = document.querySelector("#connection-status");
const lastEventEl = document.querySelector("#last-event");
const formEl = document.querySelector("#config-form");
const saveButtonEl = document.querySelector("#save-config");
const saveStatusEl = document.querySelector("#save-status");
const connectorTypeEl = document.querySelector("#connector-type");
const connectorUrlEl = document.querySelector("#connector-url");
const connectorAuthTokenEl = document.querySelector("#connector-auth-token");
const reconnectDelayEl = document.querySelector("#reconnect-delay-seconds");

function readConfigField(config, snakeCase, camelCase = "") {
  if (Object.prototype.hasOwnProperty.call(config, snakeCase)) {
    return config[snakeCase];
  }

  if (camelCase && Object.prototype.hasOwnProperty.call(config, camelCase)) {
    return config[camelCase];
  }

  return "";
}

function normalizeEventLabel(rawEvent) {
  const text = typeof rawEvent === "string" ? rawEvent.trim() : "";
  return text || "None";
}

function setConnectionStatus(isConnected, isInitialized = true) {
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

function setLastEvent(value) {
  if (!lastEventEl) {
    return;
  }

  lastEventEl.textContent = normalizeEventLabel(value);
}

function applyStatus(status) {
  const isConnected = Boolean(status?.is_connected ?? status?.isConnected);
  const lastEvent = status?.last_event ?? status?.lastEvent ?? "";

  setConnectionStatus(isConnected, true);
  setLastEvent(lastEvent);
}

function showSaveMessage(message, isSuccess = true) {
  if (!saveStatusEl) {
    return;
  }

  if (saveFeedbackTimer) {
    window.clearTimeout(saveFeedbackTimer);
  }

  saveStatusEl.textContent = message;
  saveStatusEl.style.color = isSuccess ? "#59d185" : "#ef6461";

  saveFeedbackTimer = window.setTimeout(() => {
    saveStatusEl.textContent = "";
  }, 2200);
}

async function loadInitialConfig() {
  const config = await invoke("get_config");
  currentConfig = typeof config === "object" && config !== null ? config : {};
  const connector =
    typeof currentConfig.connector === "object" && currentConfig.connector !== null
      ? currentConfig.connector
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

async function loadInitialStatus() {
  setConnectionStatus(false, false);
  setLastEvent("");

  const status = await invoke("get_status");
  applyStatus(status);
}

async function saveConfiguration(event) {
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

  const newConfig = {
    ...currentConfig,
    connector: {
      type: connectorTypeEl.value || "Firebase",
      url: connectorUrlEl.value.trim(),
      auth_token: connectorAuthTokenEl.value || null,
    },
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

async function initialize() {
  try {
    await loadInitialConfig();
    await loadInitialStatus();

    await listen("status-update", (event) => {
      if (event?.payload && typeof event.payload === "object") {
        applyStatus(event.payload);
      }
    });
  } catch (error) {
    console.error("Failed to initialize frontend", error);
    showSaveMessage("Failed to initialize app state", false);
  }
}

if (formEl) {
  formEl.addEventListener("submit", saveConfiguration);
}

window.addEventListener("DOMContentLoaded", initialize);
