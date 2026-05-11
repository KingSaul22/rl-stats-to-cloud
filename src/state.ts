import type { AppConfig, StatusPayload } from "./schemas";
import { CONSTANTS } from "./constants";

// ============================================================================
// DEFAULT STATE VALUES
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

// ============================================================================
// CENTRALIZED MUTABLE STATE OBJECT
// ============================================================================

export const state: { config: AppConfig; status: StatusPayload } = {
  config: { ...DEFAULT_CONFIG },
  status: { ...DEFAULT_STATUS },
};
