import { invoke } from "@tauri-apps/api/core";
import type { InvokeArgs } from "@tauri-apps/api/core";

import { COMMANDS } from "./constants";

import type { AppConfig, StatusPayload } from "./schemas";
import { parseAppConfig, parseStatusPayload } from "./schemas";

// ============================================================================
// COMMAND TYPES
// ============================================================================

type Command = typeof COMMANDS[keyof typeof COMMANDS];

// ============================================================================
// ERROR NORMALIZATION
// ============================================================================

function formatUnknownError(error: unknown): string {
  if (error instanceof Error) {
    return error.message;
  }

  if (typeof error === "string") {
    return error;
  }

  try {
    return JSON.stringify(error);
  } catch {
    return Object.prototype.toString.call(error);
  }
}

// ============================================================================
// SAFE TAURI INVOCATION
// ============================================================================

async function invokeSafe<T>(
  command: Command,
  args?: InvokeArgs
): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    const details = formatUnknownError(error);

    const wrapped = new Error(
    `${command} failed: ${details}`
    );

    (wrapped as Error & { cause?: unknown }).cause = error;

    throw wrapped;
  }
}

// ============================================================================
// PUBLIC API LAYER
// ============================================================================

export const api = {
  async getConfig(): Promise<AppConfig> {
    const raw = await invokeSafe<unknown>(COMMANDS.GET_CONFIG);
    return parseAppConfig(raw);
  },

  async saveConfig(config: AppConfig): Promise<void> {
    await invokeSafe<void>(
      COMMANDS.SAVE_CONFIG,
      { newConfig: config }
    );
  },

  async getStatus(): Promise<StatusPayload> {
    const raw = await invokeSafe<unknown>(COMMANDS.GET_STATUS);
    return parseStatusPayload(raw);
  },
} as const;