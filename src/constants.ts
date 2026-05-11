// ============================================================================
// COMMANDS & CONSTANTS
// ============================================================================

export const COMMANDS = {
  GET_CONFIG: "get_config",
  SAVE_CONFIG: "save_config",
  GET_STATUS: "get_status",
} as const;

export const CONSTANTS = {
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
