import { invoke } from "@tauri-apps/api/core";

/**
 * The only module allowed to call Tauri's `invoke()` (ARCHITECTURE.md §14).
 *
 * Components never import `@tauri-apps/api` directly and never construct a command name
 * by hand. Each function maps one narrow command to a typed result and turns an IPC
 * failure into an {@link IpcError}, so the UI never renders an untyped rejection.
 */

/** Runtime information returned by `get_app_info` (ARCHITECTURE.md §13). */
export interface AppInfo {
  version: string;
  build: string;
  logPath: string;
  dataDir: string;
  stateDir: string;
}

/** A failed IPC call, with the command name preserved for diagnostics. */
export class IpcError extends Error {
  readonly command: string;

  constructor(command: string, message: string) {
    super(message);
    this.name = "IpcError";
    this.command = command;
  }
}

function toIpcError(command: string, error: unknown): IpcError {
  const message = error instanceof Error ? error.message : String(error);
  return new IpcError(command, `${command} failed: ${message}`);
}

/** Application version, build id, and the standard Morphix storage locations. */
export async function getAppInfo(): Promise<AppInfo> {
  try {
    return await invoke<AppInfo>("get_app_info");
  } catch (error) {
    throw toIpcError("get_app_info", error);
  }
}
