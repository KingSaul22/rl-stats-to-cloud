import { CONSTANTS } from "./constants";
import type { AppConfig, StatusPayload } from "./schemas";

// ============================================================================
// MODULE SCOPE
// ============================================================================

let saveFeedbackTimer: ReturnType<typeof setTimeout> | undefined;

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

export function renderConnectionStatus(isConnected: boolean): void {
  const el = requiredElement<HTMLElement>(
    CONSTANTS.UI_SELECTORS.CONNECTION_STATUS,
    "connection-status"
  );

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

export function renderConnectionStatusConnecting(): void {
  const el = requiredElement<HTMLElement>(
    CONSTANTS.UI_SELECTORS.CONNECTION_STATUS,
    "connection-status"
  );

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

export function renderStatus(status: StatusPayload): void {
  renderConnectionStatus(status.isConnected);
  renderLastEvent(status.lastEvent);
}

export function renderConfigForm(config: AppConfig): void {
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

  typeEl.value = config.connector.type || CONSTANTS.CONNECTOR_TYPES.FIREBASE;
  urlEl.value = config.connector.url;
  authEl.value = config.connector.authToken || "";
  delayEl.value = String(config.reconnectDelaySeconds);
}

export function renderSaveMessage(message: string, isSuccess: boolean): void {
  const el = requiredElement<HTMLElement>(CONSTANTS.UI_SELECTORS.SAVE_STATUS, "save-status");
  el.textContent = message;
  el.style.color = isSuccess ? "#59d185" : "#ef6461";
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
    const el = requiredElement<HTMLElement>(CONSTANTS.UI_SELECTORS.SAVE_STATUS, "save-status");
    el.textContent = "";
  }, CONSTANTS.DEFAULT_DELAYS.MESSAGE_FADE);
}
