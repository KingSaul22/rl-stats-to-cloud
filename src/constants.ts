// ============================================================================
// COMMANDS & CONSTANTS
// ============================================================================

export const COMMANDS = {
  GET_CONFIG: "get_config",
  SAVE_CONFIG: "save_config",
  GET_STATUS: "get_status",
  SHUTDOWN_DAEMON: "shutdown_daemon",
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
    SHUTDOWN_SUCCESS: "Daemon shutdown request sent.",
    SHUTDOWN_ERROR: "Failed to shut down daemon",
    INIT_ERROR: "Failed to initialize app state",
    CONNECTING: "⚪ Connecting",
    CONNECTED: "🟢 Running",
    DISCONNECTED: "🔴 Offline",
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
    CONNECTOR_API_KEY: "#connector-api-key",
    CONNECTOR_EMAIL: "#connector-email",
    CONNECTOR_PASSWORD: "#connector-password",
    RECONNECT_DELAY: "#reconnect-delay-seconds",
    SHUTDOWN_BUTTON: "#shutdown-daemon",
    OFFLINE_CONFIG_BUTTON: "#offline-config-btn",
  },
  CSS_CLASSES: {
    STATUS_CONNECTING: "status-connecting",
    STATUS_CONNECTED: "status-connected",
    STATUS_DISCONNECTED: "status-disconnected",
  },
} as const;
