import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core"; // for reconnect command (Tauri v2)
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
// OFFLINE PANEL TOGGLE
// ============================================================================

function toggleOfflinePanel(connected: boolean): void {
  const normalView = document.getElementById("normal-view");
  const offlinePanel = document.getElementById("offline-panel");

  if (!normalView || !offlinePanel) {
    return; // silent guard if not yet in DOM
  }

  if (connected) {
    normalView.style.display = "block";
    offlinePanel.style.display = "none";
  } else {
    normalView.style.display = "none";
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
// OFFLINE PANEL BUTTON HANDLERS
// ============================================================================

function setupOfflinePanelButtons(): void {
  const reconnectBtn = document.getElementById("reconnect-btn");
  const configBtn = document.getElementById("offline-config-btn");

  if (reconnectBtn) {
    reconnectBtn.addEventListener("click", async () => {
      try {
        // Replace with your actual Tauri command when ready
        await invoke("reconnect_daemon");
      } catch (error) {
        logError("reconnect_daemon", error);
      }
    });
  }

  if (configBtn) {
    configBtn.addEventListener("click", () => {
      // Switch back to normal view and scroll to configuration
      toggleOfflinePanel(false); // force show config (but we need to show normal view)
      // Actually we want to show normal view; so call with true and then scroll
      toggleOfflinePanel(true);
      document.getElementById("config-title")?.scrollIntoView({ behavior: "smooth" });
    });
  }
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

    setupOfflinePanelButtons();

    void initialize();
  } catch (error) {
    logError("DOMContentLoaded", error);
  }
});

window.addEventListener("beforeunload", () => {
  cleanup();
});