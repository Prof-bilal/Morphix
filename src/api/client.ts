/**
 * The only module allowed to call Tauri's `invoke()` (ARCHITECTURE.md §14).
 *
 * Components never import `@tauri-apps/api` directly and never construct a command name
 * by hand. Each function here maps one narrow command to a typed result, and is the place
 * where an IPC error string becomes a typed failure.
 *
 * P0-06 lands this boundary deliberately empty; P0-07 adds `getAppInfo`.
 */

export {};
