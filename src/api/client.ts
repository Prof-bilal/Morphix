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

/** Support verdict levels (MVP.md AC-04). */
export type SupportLevel = "supported" | "unverified" | "unsupported";

/** Closed refusal/warning reason codes (morphix-core `SupportReason`). */
export type SupportReason =
  | "NOT_LINUX"
  | "RUNNING_AS_ROOT"
  | "NO_SESSION_BUS"
  | "NO_PLASMA"
  | "PLASMA_VERSION_UNSUPPORTED"
  | "PLASMA_VERSION_UNKNOWN"
  | "KWIN_UNAVAILABLE"
  | "KGLOBALACCEL_UNAVAILABLE"
  | "KDEGLOBALS_UNREADABLE"
  | "KWINRC_UNREADABLE"
  | "DATA_DIR_UNWRITABLE"
  | "PLASMA_MINOR_UNVALIDATED"
  | "SESSION_TYPE_UNVALIDATED"
  | "ARCHITECTURE_UNVALIDATED"
  | "MULTI_SCREEN_UNVALIDATED";

export interface SupportReport {
  level: SupportLevel;
  reasons: SupportReason[];
  summary: string;
}

export interface DistroInfo {
  id: string | null;
  name: string | null;
  prettyName: string | null;
  versionId: string | null;
  idLike: string[];
}

export type SessionType = "wayland" | "x11" | "tty" | "unknown";

export type PackageManager = "pacman" | "dpkg" | "rpm" | "zypper" | "unknown";

/** Environment and support verdict returned by `get_system_status`. */
export interface SystemStatus {
  supported: boolean;
  support: SupportReport;
  distro: DistroInfo;
  packageManager: PackageManager;
  plasmaVersion: string | null;
  plasmashellRunning: boolean;
  sessionType: SessionType;
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

/**
 * Current support verdict and detected environment.
 *
 * `refresh` false may return a result cached by the core for up to 30 seconds; Re-check
 * passes true.
 */
export async function getSystemStatus(refresh = false): Promise<SystemStatus> {
  try {
    return await invoke<SystemStatus>("get_system_status", { refresh });
  } catch (error) {
    throw toIpcError("get_system_status", error);
  }
}
