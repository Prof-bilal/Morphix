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
  readonly code?: string;
  readonly detail?: unknown;

  constructor(command: string, message: string, code?: string, detail?: unknown) {
    super(message);
    this.name = "IpcError";
    this.command = command;
    this.code = code;
    this.detail = detail;
  }
}

function toIpcError(command: string, error: unknown): IpcError {
  if (typeof error === "object" && error !== null && "message" in error) {
    const structured = error as { code?: unknown; message: unknown; detail?: unknown };
    if (typeof structured.message === "string") {
      const code = typeof structured.code === "string" ? structured.code : undefined;
      const prefix = code === undefined ? "" : `${code}: `;
      return new IpcError(
        command,
        `${command} failed: ${prefix}${structured.message}`,
        code,
        structured.detail,
      );
    }
  }
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

/** Where an available pack came from. */
export type PackSource = "bundled" | "local";

/** Whether a pack currently validates. */
export type PackStatus = "valid" | "invalid";

/** Where the pinned integrity hash came from. */
export type IntegritySource = "declared" | "generated_local";

/** A field-level validation problem. */
export interface ValidationIssue {
  code: string;
  message: string;
  path?: string;
  field?: string;
}

/** Catalog card data returned by `list_packs`. */
export interface PackSummary {
  id: string;
  name: string;
  version?: string;
  summary: string;
  source: PackSource;
  status: PackStatus;
  compatibility?: string;
  capabilities: string[];
  icon?: string;
  applied: boolean;
  integritySource: IntegritySource;
  issueCount: number;
}

/** One declared asset in the detail view. */
export interface AssetView {
  kind: string;
  key: string;
  path: string;
  installedName: string;
  isDirectory: boolean;
  bytes: number;
}

/** One preview image. */
export interface PreviewView {
  caption: string;
  image: string;
  absolutePath: string;
}

/** One declared dependency. */
export interface DependencyView {
  id: string;
  version: string;
}

/** One declared change, by capability and target kind (AC-08). */
export interface DeclaredChange {
  capability: string;
  kind: string;
  target: string;
}

/** Full pack detail returned by `get_pack_detail`. */
export interface PackDetail {
  summary: PackSummary;
  description: string;
  authors: string[];
  license: string;
  homepage?: string;
  attribution?: string;
  assets: AssetView[];
  declaredChanges: DeclaredChange[];
  previews: PreviewView[];
  dependencies: DependencyView[];
  issues: ValidationIssue[];
  manifestHash?: string;
  contentHash: string;
}

/** Lists bundled, installed, and invalid packs. */
export async function listPacks(): Promise<PackSummary[]> {
  try {
    return await invoke<PackSummary[]>("list_packs");
  } catch (error) {
    throw toIpcError("list_packs", error);
  }
}

/** Returns one pack's detail, including the declared change set. */
export async function getPackDetail(packId: string): Promise<PackDetail> {
  try {
    return await invoke<PackDetail>("get_pack_detail", { packId });
  } catch (error) {
    throw toIpcError("get_pack_detail", error);
  }
}

/**
 * Validates and installs a pack from an OS-dialog directory.
 *
 * This is the only path-accepting pack call; the UI never asks for a typed path.
 */
export async function installLocalPack(directory: string): Promise<PackSummary> {
  try {
    return await invoke<PackSummary>("install_local_pack", { directory });
  } catch (error) {
    throw toIpcError("install_local_pack", error);
  }
}
