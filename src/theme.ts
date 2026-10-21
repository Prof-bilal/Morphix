/**
 * Theme preference (UI_SPEC.md §5.7). The choice is local-only: it sets `data-theme` on
 * the root element, which selects the token set in `tokens.css`. No IPC, no Plasma.
 */

export type ThemePreference = "system" | "light" | "dark";

const STORAGE_KEY = "morphix.theme";

const PREFERENCES: readonly ThemePreference[] = ["system", "light", "dark"];

/** Previously saved preference, defaulting to the system scheme on any unknown value. */
export function readThemePreference(): ThemePreference {
  try {
    const saved = window.localStorage.getItem(STORAGE_KEY);
    return PREFERENCES.includes(saved as ThemePreference)
      ? (saved as ThemePreference)
      : "system";
  } catch {
    return "system";
  }
}

/**
 * Applies the preference: an explicit choice sets `data-theme`, while `system` removes
 * the attribute so the `prefers-color-scheme` media query decides.
 */
export function applyThemePreference(preference: ThemePreference): void {
  if (preference === "system") {
    document.documentElement.removeAttribute("data-theme");
  } else {
    document.documentElement.setAttribute("data-theme", preference);
  }
  try {
    window.localStorage.setItem(STORAGE_KEY, preference);
  } catch {
    // A full or blocked store must never break rendering; the attribute above already
    // applied for this session.
  }
}
