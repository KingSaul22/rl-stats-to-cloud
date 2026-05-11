import { listen } from "@tauri-apps/api/event";
import { CONSTANTS } from "./constants";
import type { AppConfig } from "./schemas";
import { parseStatusPayload } from "./schemas";
import { state } from "./state";
import { api } from "./api";
import {
  requiredElement,
  renderConnectionStatusConnecting,
  renderConfigForm,
  renderSaveMessage,
  renderStatus,
  resetSaveMessageTimeout,
  startSaveMessageTimeout,
} from "./ui";

// ============================================================================
// MODULE SCOPE
// ============================================================================

let unlistenFn: (() => void) | undefined;

// ============================================================================
// ERROR HANDLING & UTILITIES
// ============================================================================

function logError(context: string, error: unknown): void {
  const errorMsg = error instanceof Error ? error.message : String(error);
  console.error(`[${context}] ${errorMsg}`);
}

// ============================================================================
// LIFECYCLE & EVENT HANDLERS
// ============================================================================

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

  const saveBtn = requiredElement<HTMLButtonElement>(
    CONSTANTS.UI_SELECTORS.SAVE_BUTTON,
    "save-button"
  );
  const typeEl = requiredElement<HTMLSelectElement>(
    CONSTANTS.UI_SELECTORS.CONNECTOR_TYPE,
    "connector-type"
  );
  const urlEl = requiredElement<HTMLInputElement>(
    CONSTANTS.UI_SELECTORS.CONNECTOR_URL,
    "connector-url"
  );
  const authEl = requiredElement<HTMLInputElement>(
    CONSTANTS.UI_SELECTORS.CONNECTOR_AUTH_TOKEN,
    "connector-auth-token"
  );
  const delayEl = requiredElement<HTMLInputElement>(
    CONSTANTS.UI_SELECTORS.RECONNECT_DELAY,
    "reconnect-delay"
  );

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
// FINAL ASSEMBLY & EVENT LISTENERS
// ============================================================================

window.addEventListener("DOMContentLoaded", () => {
  const formEl = requiredElement<HTMLFormElement>(
    CONSTANTS.UI_SELECTORS.CONFIG_FORM,
    "config-form"
  );
  formEl.addEventListener("submit", handleSaveConfig);

  void initialize();
});

window.addEventListener("beforeunload", () => {
  cleanup();
});
