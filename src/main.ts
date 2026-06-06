import { listen } from "@tauri-apps/api/event";
import { CONSTANTS } from "./constants";
import type { AppConfig, StatusPayload } from "./schemas";
import { parseStatusPayload } from "./schemas";
import { api } from "./api";
import {
  initializeDOMCache,
  requiredElement,
  renderConnectionState,
  renderConfigForm,
  renderSaveMessage,
  renderStatus,
  resetSaveMessageTimeout,
  startSaveMessageTimeout,
  setFormButtonState,
  getFormValues,
} from "./ui";

// ============================================================================
// CENTRALIZED MUTABLE STATE
// ============================================================================

const DEFAULT_CONFIG: AppConfig = {
  connector: {
    type: "",
    url: "",
    apiKey: "",
    email: "",
    password: "",
  },
  rememberPassword: false,
  reconnectDelaySeconds: CONSTANTS.DEFAULT_DELAYS.RECONNECT,
  isHeadless: false,
  websocketUrl: "ws://localhost:1420",
  uiSyncPort: 54321,
};

const DEFAULT_STATUS: StatusPayload = {
  isConnected: false,
  lastEvent: "",
};

const state: { config: AppConfig; status: StatusPayload } = {
  config: { ...DEFAULT_CONFIG },
  status: { ...DEFAULT_STATUS },
};

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
// OFFLINE PANEL TOGGLE (only status area)
// ============================================================================

function toggleOfflinePanel(connected: boolean): void {
  const normalStatus = document.getElementById("normal-status-panel");
  const offlinePanel = document.getElementById("offline-panel");

  if (!normalStatus || !offlinePanel) return;

  if (connected) {
    normalStatus.style.display = "block";
    offlinePanel.style.display = "none";
  } else {
    normalStatus.style.display = "none";
    offlinePanel.style.display = "block";
  }
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
  updateUIWithStatus(status);
}

function updateUIWithStatus(status: StatusPayload): void {
  renderStatus(status);
  toggleOfflinePanel(status.isConnected);
}

function hasRequiredFirebaseCredentials(config: AppConfig): boolean {
  return (
    config.connector.apiKey.trim().length > 0 &&
    config.connector.email.trim().length > 0 &&
    config.connector.password.trim().length > 0
  );
}

async function handleSaveConfig(event: Event): Promise<void> {
  event.preventDefault();

  const saveBtn = requiredElement<HTMLButtonElement>(
    CONSTANTS.UI_SELECTORS.SAVE_BUTTON,
    "save-button"
  );
  const previousButtonLabel = saveBtn.textContent;
  setFormButtonState(true, CONSTANTS.UI_MESSAGES.SAVING);

  const newConfig = getFormValues();

  if (!newConfig) {
    logError("handleSaveConfig", "Failed to compile configuration from form UI elements.");
    renderSaveMessage(CONSTANTS.UI_MESSAGES.SAVED_ERROR, false);
    setFormButtonState(false, previousButtonLabel || "Save Configuration");
    return;
  }

  if (!Number.isFinite(newConfig.reconnectDelaySeconds) || newConfig.reconnectDelaySeconds <= 0) {
    logError("handleSaveConfig", `Invalid reconnect delay value: ${newConfig.reconnectDelaySeconds}`);
    renderSaveMessage(CONSTANTS.UI_MESSAGES.SAVED_ERROR, false);
    setFormButtonState(false, previousButtonLabel || "Save Configuration");
    return;
  }

  if (!hasRequiredFirebaseCredentials(newConfig)) {
    logError("handleSaveConfig", "Missing required Firebase credentials.");
    renderSaveMessage("Firebase Api Key, email, and password are required.", false);
    setFormButtonState(false, previousButtonLabel || "Save Configuration");
    return;
  }

  try {
    await api.saveConfig(newConfig);
    state.config = newConfig;
    renderConfigForm(newConfig);
    renderSaveMessage(CONSTANTS.UI_MESSAGES.SAVED_SUCCESS, true);
  } catch (error) {
    logError("handleSaveConfig", error);
    renderSaveMessage(CONSTANTS.UI_MESSAGES.SAVED_ERROR, false);
  } finally {
    startSaveMessageTimeout();
    setFormButtonState(false, previousButtonLabel || "Save Configuration");
  }
}

async function registerStatusListener(): Promise<void> {
  unlistenFn = await listen<unknown>("status-update", (event) => {
    if (event?.payload && typeof event.payload === "object") {
      try {
        const status = parseStatusPayload(event.payload);
        state.status = status;
        updateUIWithStatus(status);
      } catch (error) {
        logError("registerStatusListener: parseStatusPayload", error);
      }
    }
  });
}

async function initialize(): Promise<void> {
  try {
    await registerStatusListener();
    renderConnectionState(CONSTANTS.CONNECTION_STATES.CONNECTING);

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
// CONTROL PANEL BUTTON HANDLERS
// ============================================================================

function setupControlPanelButtons(): void {
  const shutdownBtn = requiredElement<HTMLButtonElement>(
    CONSTANTS.UI_SELECTORS.SHUTDOWN_BUTTON,
    "shutdown-daemon"
  );
  const configBtn = requiredElement<HTMLButtonElement>(
    CONSTANTS.UI_SELECTORS.OFFLINE_CONFIG_BUTTON,
    "offline-config-btn"
  );

  shutdownBtn.addEventListener("click", async () => {
    const confirmed = window.confirm(
      "Are you sure you want to shut down the background daemon?"
    );
    if (!confirmed) {
      return;
    }

    shutdownBtn.disabled = true;

    try {
      await api.shutdownDaemon();
      renderSaveMessage(CONSTANTS.UI_MESSAGES.SHUTDOWN_SUCCESS, true);
    } catch (error) {
      logError("shutdownDaemon", error);
      renderSaveMessage(CONSTANTS.UI_MESSAGES.SHUTDOWN_ERROR, false);
    } finally {
      startSaveMessageTimeout();
      shutdownBtn.disabled = false;
    }
  });

  configBtn.addEventListener("click", () => {
    document.getElementById("config-title")?.scrollIntoView({ behavior: "smooth" });
  });
}

// ============================================================================
// FINAL ASSEMBLY & EVENT LISTENERS
// ============================================================================

window.addEventListener("DOMContentLoaded", () => {
  try {
    initializeDOMCache();

    const formEl = requiredElement<HTMLFormElement>(
      CONSTANTS.UI_SELECTORS.CONFIG_FORM,
      "config-form"
    );
    formEl.addEventListener("submit", handleSaveConfig);

    setupControlPanelButtons();

    void initialize();
  } catch (error) {
    logError("DOMContentLoaded", error);
  }
});

window.addEventListener("beforeunload", () => {
  cleanup();
});