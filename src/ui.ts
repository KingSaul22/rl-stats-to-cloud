import { CONSTANTS } from "./constants";
import type { AppConfig, StatusPayload } from "./schemas";

// ============================================================================
// MODULE SCOPE & DOM ELEMENT CACHING
// ============================================================================

let saveFeedbackTimer: ReturnType<typeof setTimeout> | undefined;

const DOM: {
  connectionStatus: HTMLElement | null;
  lastEvent: HTMLElement | null;
  configForm: HTMLFormElement | null;
  saveButton: HTMLButtonElement | null;
  saveStatus: HTMLElement | null;
  connectorType: HTMLSelectElement | null;
  connectorUrl: HTMLInputElement | null;
  connectorApiKey: HTMLInputElement | null;
  connectorEmail: HTMLInputElement | null;
  connectorPassword: HTMLInputElement | null;
  rememberPassword: HTMLInputElement | null;
  reconnectDelay: HTMLInputElement | null;
  websocketUrl: HTMLInputElement | null;
  uiSyncPort: HTMLInputElement | null;
  isHeadless: HTMLInputElement | null;
} = {
  connectionStatus: null,
  lastEvent: null,
  configForm: null,
  saveButton: null,
  saveStatus: null,
  connectorType: null,
  connectorUrl: null,
  connectorApiKey: null,
  connectorEmail: null,
  connectorPassword: null,
  rememberPassword: null,
  reconnectDelay: null,
  websocketUrl: null,
  uiSyncPort: null,
  isHeadless: null,
};

/**
 * Initialize DOM element cache. Call once from DOMContentLoaded.
 */
export function initializeDOMCache(): void {
  DOM.connectionStatus = document.querySelector<HTMLElement>(CONSTANTS.UI_SELECTORS.CONNECTION_STATUS);
  DOM.lastEvent = document.querySelector<HTMLElement>(CONSTANTS.UI_SELECTORS.LAST_EVENT);
  DOM.configForm = document.querySelector<HTMLFormElement>(CONSTANTS.UI_SELECTORS.CONFIG_FORM);
  DOM.saveButton = document.querySelector<HTMLButtonElement>(CONSTANTS.UI_SELECTORS.SAVE_BUTTON);
  DOM.saveStatus = document.querySelector<HTMLElement>(CONSTANTS.UI_SELECTORS.SAVE_STATUS);
  DOM.connectorType = document.querySelector<HTMLSelectElement>(CONSTANTS.UI_SELECTORS.CONNECTOR_TYPE);
  DOM.connectorUrl = document.querySelector<HTMLInputElement>(CONSTANTS.UI_SELECTORS.CONNECTOR_URL);
  DOM.connectorApiKey = document.querySelector<HTMLInputElement>(CONSTANTS.UI_SELECTORS.CONNECTOR_API_KEY);
  DOM.connectorEmail = document.querySelector<HTMLInputElement>(CONSTANTS.UI_SELECTORS.CONNECTOR_EMAIL);
  DOM.connectorPassword = document.querySelector<HTMLInputElement>(CONSTANTS.UI_SELECTORS.CONNECTOR_PASSWORD);
  DOM.rememberPassword = document.querySelector<HTMLInputElement>(CONSTANTS.UI_SELECTORS.REMEMBER_PASSWORD);
  DOM.reconnectDelay = document.querySelector<HTMLInputElement>(CONSTANTS.UI_SELECTORS.RECONNECT_DELAY);

  DOM.websocketUrl = document.querySelector<HTMLInputElement>("#websocket-url");
  DOM.uiSyncPort = document.querySelector<HTMLInputElement>("#ui-sync-port");
  DOM.isHeadless = document.querySelector<HTMLInputElement>("#is-headless");

  const missing = Object.entries(DOM)
    .filter(([_k, v]) => v === null)
    .map(([k]) => k);

  if (missing.length > 0) {
    throw new Error(`Missing DOM elements: ${missing.join(", ")}`);
  }
}

// ============================================================================
// DOM SAFETY HELPER
// ============================================================================

export function requiredElement<T extends Element>(selector: string, context = ""): T {
  const element = document.querySelector<T>(selector);
  if (!element) {
    throw new Error(`DOM element not found: ${selector}${context ? ` (${context})` : ""}`);
  }
  return element;
}

// ============================================================================
// UI RENDERING FUNCTIONS
// ============================================================================

export function renderConnectionState(state: "connecting" | "connected" | "disconnected"): void {
  if (!DOM.connectionStatus) return;

  DOM.connectionStatus.classList.remove(
    CONSTANTS.CSS_CLASSES.STATUS_CONNECTING,
    CONSTANTS.CSS_CLASSES.STATUS_CONNECTED,
    CONSTANTS.CSS_CLASSES.STATUS_DISCONNECTED
  );

  switch (state) {
    case CONSTANTS.CONNECTION_STATES.CONNECTING:
      DOM.connectionStatus.textContent = CONSTANTS.UI_MESSAGES.CONNECTING;
      DOM.connectionStatus.classList.add(CONSTANTS.CSS_CLASSES.STATUS_CONNECTING);
      break;
    case CONSTANTS.CONNECTION_STATES.CONNECTED:
      DOM.connectionStatus.textContent = CONSTANTS.UI_MESSAGES.CONNECTED;
      DOM.connectionStatus.classList.add(CONSTANTS.CSS_CLASSES.STATUS_CONNECTED);
      break;
    case CONSTANTS.CONNECTION_STATES.DISCONNECTED:
      DOM.connectionStatus.textContent = CONSTANTS.UI_MESSAGES.DISCONNECTED;
      DOM.connectionStatus.classList.add(CONSTANTS.CSS_CLASSES.STATUS_DISCONNECTED);
      break;
  }
}

function renderLastEvent(lastEvent: string): void {
  if (!DOM.lastEvent) return;
  DOM.lastEvent.textContent = lastEvent.trim() || CONSTANTS.UI_MESSAGES.NONE_EVENT;
}

export function renderStatus(status: StatusPayload): void {
  const connectionState = status.isConnected
    ? CONSTANTS.CONNECTION_STATES.CONNECTED
    : CONSTANTS.CONNECTION_STATES.DISCONNECTED;
  renderConnectionState(connectionState);
  renderLastEvent(status.lastEvent);
}

export function renderConfigForm(config: AppConfig): void {
  if (
    !DOM.connectorType || !DOM.connectorUrl || !DOM.connectorApiKey ||
    !DOM.connectorEmail || !DOM.connectorPassword ||
    !DOM.rememberPassword || !DOM.reconnectDelay || !DOM.websocketUrl || !DOM.uiSyncPort || !DOM.isHeadless
  ) {
    return;
  }

  DOM.connectorType.value = config.connector.type || CONSTANTS.CONNECTOR_TYPES.FIREBASE;
  DOM.connectorUrl.value = config.connector.url;
  DOM.connectorApiKey.value = config.connector.apiKey;
  DOM.connectorEmail.value = config.connector.email;
  DOM.connectorPassword.value = config.connector.password;
  DOM.rememberPassword.checked = config.rememberPassword;
  DOM.reconnectDelay.value = String(config.reconnectDelaySeconds);
  DOM.websocketUrl.value = config.websocketUrl;
  DOM.uiSyncPort.value = String(config.uiSyncPort);
  DOM.isHeadless.checked = config.isHeadless;
}

export function renderSaveMessage(message: string, isSuccess: boolean): void {
  if (!DOM.saveStatus) return;

  DOM.saveStatus.textContent = message;
  DOM.saveStatus.classList.remove(
    CONSTANTS.CSS_CLASSES.STATUS_CONNECTED,
    CONSTANTS.CSS_CLASSES.STATUS_DISCONNECTED
  );
  DOM.saveStatus.classList.add(
    isSuccess ? CONSTANTS.CSS_CLASSES.STATUS_CONNECTED : CONSTANTS.CSS_CLASSES.STATUS_DISCONNECTED
  );
  DOM.saveStatus.style.opacity = "1";
}

// ============================================================================
// FEEDBACK MESSAGE TIMEOUT MANAGEMENT
// ============================================================================

export function resetSaveMessageTimeout(): void {
  if (saveFeedbackTimer !== undefined) {
    clearTimeout(saveFeedbackTimer);
  }
}

export function startSaveMessageTimeout(): void {
  resetSaveMessageTimeout();
  saveFeedbackTimer = setTimeout(() => {
    if (DOM.saveStatus) {
      DOM.saveStatus.style.opacity = "0";
    }
  }, CONSTANTS.DEFAULT_DELAYS.MESSAGE_FADE);
}

// ============================================================================
// FORM FIELD ACCESS HELPERS
// ============================================================================

/**
 * Extract clean, typed configuration objects directly from the boundary inputs.
 */
export function getFormValues(): AppConfig | null {
  if (
    !DOM.connectorType || !DOM.connectorUrl || !DOM.connectorApiKey ||
    !DOM.connectorEmail || !DOM.connectorPassword ||
    !DOM.rememberPassword || !DOM.reconnectDelay || !DOM.websocketUrl || !DOM.uiSyncPort || !DOM.isHeadless
  ) {
    return null;
  }

  return {
    connector: {
      type: DOM.connectorType.value,
      url: DOM.connectorUrl.value.trim(),
      apiKey: DOM.connectorApiKey.value.trim(),
      email: DOM.connectorEmail.value.trim(),
      password: DOM.connectorPassword.value,
    },
    rememberPassword: DOM.rememberPassword.checked,
    reconnectDelaySeconds: Math.floor(Number(DOM.reconnectDelay.value)),
    isHeadless: DOM.isHeadless.checked,
    websocketUrl: DOM.websocketUrl.value.trim(),
    uiSyncPort: Math.floor(Number(DOM.uiSyncPort.value)),
  };
}

export function setFormButtonState(disabled: boolean, textContent: string | null): void {
  if (!DOM.saveButton) return;

  DOM.saveButton.disabled = disabled;
  if (textContent !== null) {
    DOM.saveButton.textContent = textContent;
  }
}