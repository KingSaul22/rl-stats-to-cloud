import { invoke } from "@tauri-apps/api/core";
import { COMMANDS } from "./constants";
import type { AppConfig, StatusPayload } from "./schemas";
import { parseAppConfig, parseStatusPayload } from "./schemas";

// ============================================================================
// API LAYER WITH EXPLICIT COMMAND CONSTANTS
// ============================================================================

export const api = {
  async getConfig(): Promise<AppConfig> {
    const data = await invoke<unknown>(COMMANDS.GET_CONFIG);
    return parseAppConfig(data);
  },

  async saveConfig(config: AppConfig): Promise<void> {
    await invoke<void>(COMMANDS.SAVE_CONFIG, { newConfig: config });
  },

  async getStatus(): Promise<StatusPayload> {
    const data = await invoke<unknown>(COMMANDS.GET_STATUS);
    return parseStatusPayload(data);
  },
};
