import { CONSTANTS } from "./constants";
import type { AppConfig, StatusPayload } from "./schemas";

// ============================================================================
// MODULE SCOPE & DOM ELEMENT CACHING
// ============================================================================

let saveFeedbackTimer: ReturnType<typeof setTimeout> | undefined;

// DOM elements cached at module initialization
const DOM: {
  connectionStatus: HTMLElement | null;
  lastEvent: HTMLElement | null;
  configForm: HTMLFormElement | null;
  saveButton: HTMLButtonElement | null;
  saveStatus: HTMLElement | null;
  connectorType: HTMLSelectElement | null;
  connectorUrl: HTMLInputElement | null;
  connectorAuthToken: HTMLInputElement | null;
  reconnectDelay: HTMLInputElement | null;
} = {
  connectionStatus: null,
  lastEvent: null,
  configForm: null,
  saveButton: null,
  saveStatus: null,
  connectorType: null,
  connectorUrl: null,
  connectorAuthToken: null,
  reconnectDelay: null,
};

/**
 * Initialize DOM element cache. Call once from DOMContentLoaded.
 */
export function initializeDOMCache(): void {
  DOM.connectionStatus = document.querySelector<HTMLElement>(
    CONSTANTS.UI_SELECTORS.CONNECTION_STATUS
  );
  DOM.lastEvent = document.querySelector<HTMLElement>(CONSTANTS.UI_SELECTORS.LAST_EVENT);
  DOM.configForm = document.querySelector<HTMLFormElement>(CONSTANTS.UI_SELECTORS.CONFIG_FORM);
  DOM.saveButton = document.querySelector<HTMLButtonElement>(CONSTANTS.UI_SELECTORS.SAVE_BUTTON);
  DOM.saveStatus = document.querySelector<HTMLElement>(CONSTANTS.UI_SELECTORS.SAVE_STATUS);
  DOM.connectorType = document.querySelector<HTMLSelectElement>(
    CONSTANTS.UI_SELECTORS.CONNECTOR_TYPE
  );
  DOM.connectorUrl = document.querySelector<HTMLInputElement>(CONSTANTS.UI_SELECTORS.CONNECTOR_URL);
  DOM.connectorAuthToken = document.querySelector<HTMLInputElement>(
    CONSTANTS.UI_SELECTORS.CONNECTOR_AUTH_TOKEN
  );
  DOM.reconnectDelay = document.querySelector<HTMLInputElement>(
    CONSTANTS.UI_SELECTORS.RECONNECT_DELAY
  );

  // Verify all required elements exist
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

/**
 * Render connection state with unified styling.
 * @param state - "connecting" | "connected" | "disconnected"
 */
export function renderConnectionState(
  state: "connecting" | "connected" | "disconnected"
): void {
  if (!DOM.connectionStatus) return;

  // Remove all status classes
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

  const normalized = lastEvent.trim() || CONSTANTS.UI_MESSAGES.NONE_EVENT;
  DOM.lastEvent.textContent = normalized;
}

export function renderStatus(status: StatusPayload): void {
  const connectionState = status.isConnected
    ? CONSTANTS.CONNECTION_STATES.CONNECTED
    : CONSTANTS.CONNECTION_STATES.DISCONNECTED;
  renderConnectionState(connectionState);
  renderLastEvent(status.lastEvent);
}

export function renderConfigForm(config: AppConfig): void {
  if (!DOM.connectorType || !DOM.connectorUrl || !DOM.connectorAuthToken || !DOM.reconnectDelay) {
    return;
  }

  DOM.connectorType.value = config.connector.type || CONSTANTS.CONNECTOR_TYPES.FIREBASE;
  DOM.connectorUrl.value = config.connector.url;
  DOM.connectorAuthToken.value = config.connector.authToken || "";
  DOM.reconnectDelay.value = String(config.reconnectDelaySeconds);
}

export function renderSaveMessage(message: string, isSuccess: boolean): void {
  if (!DOM.saveStatus) return;

  DOM.saveStatus.textContent = message;

  // Remove existing status classes
  DOM.saveStatus.classList.remove(
    CONSTANTS.CSS_CLASSES.STATUS_CONNECTED,
    CONSTANTS.CSS_CLASSES.STATUS_DISCONNECTED
  );

  // Apply appropriate class
  DOM.saveStatus.classList.add(
    isSuccess
      ? CONSTANTS.CSS_CLASSES.STATUS_CONNECTED
      : CONSTANTS.CSS_CLASSES.STATUS_DISCONNECTED
  );

  // Make visible
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

export function getFormValues(): {
  connectorType: string;
  connectorUrl: string;
  connectorAuthToken: string;
  reconnectDelay: number;
} | null {
  if (!DOM.connectorType || !DOM.connectorUrl || !DOM.connectorAuthToken || !DOM.reconnectDelay) {
    return null;
  }

  return {
    connectorType: DOM.connectorType.value,
    connectorUrl: DOM.connectorUrl.value,
    connectorAuthToken: DOM.connectorAuthToken.value,
    reconnectDelay: Number(DOM.reconnectDelay.value),
  };
}

export function setFormButtonState(
  disabled: boolean,
  textContent: string | null
): void {
  if (!DOM.saveButton) return;

  DOM.saveButton.disabled = disabled;
  if (textContent !== null) {
    DOM.saveButton.textContent = textContent;
  }
}
