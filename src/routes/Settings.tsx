import { useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { Link } from "react-router-dom";

import { getAppInfo } from "../api/client";
import {
  applyThemePreference,
  readThemePreference,
  type ThemePreference,
} from "../theme";
import { CopyButton } from "../components/CopyButton";
import { OfflineBadge } from "../components/OfflineBadge";
import { RoutePlaceholder } from "../components/RoutePlaceholder";

const THEME_OPTIONS: readonly { value: ThemePreference; label: string }[] = [
  { value: "system", label: "System" },
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
];

export function Settings() {
  const appInfo = useQuery({
    queryKey: ["app-info"],
    queryFn: getAppInfo,
    staleTime: 60_000,
  });
  const [theme, setTheme] = useState<ThemePreference>(readThemePreference);

  function chooseTheme(preference: ThemePreference): void {
    setTheme(preference);
    applyThemePreference(preference);
  }

  return (
    <RoutePlaceholder
      title="Settings"
      lead="Where Morphix keeps its data, and which preferences you control."
      note="P8-11: clear-history control."
    >
      <OfflineBadge />
      <section className="info-card" aria-labelledby="app-info-title">
        <h2 id="app-info-title">Application</h2>
        {appInfo.isPending ? (
          <p role="status">Reading application information…</p>
        ) : null}
        {appInfo.isError ? (
          <p role="alert">
            Could not read application information: {appInfo.error.message}
          </p>
        ) : null}
        {appInfo.isSuccess ? (
          <dl className="info-list">
            <div className="info-list__row">
              <dt>Version</dt>
              <dd>{appInfo.data.version}</dd>
            </div>
            <div className="info-list__row">
              <dt>Build</dt>
              <dd>{appInfo.data.build}</dd>
            </div>
            <div className="info-list__row">
              <dt>Log file</dt>
              <dd className="code">{appInfo.data.logPath}</dd>
              <CopyButton text={appInfo.data.logPath} label="log path" />
            </div>
            <div className="info-list__row">
              <dt>Data directory</dt>
              <dd className="code">{appInfo.data.dataDir}</dd>
              <CopyButton text={appInfo.data.dataDir} label="data path" />
            </div>
            <div className="info-list__row">
              <dt>State directory</dt>
              <dd className="code">{appInfo.data.stateDir}</dd>
              <CopyButton text={appInfo.data.stateDir} label="state path" />
            </div>
          </dl>
        ) : null}
      </section>
      <section className="info-card" aria-labelledby="appearance-title">
        <h2 id="appearance-title">Appearance</h2>
        <fieldset className="theme-picker">
          <legend>Theme</legend>
          {THEME_OPTIONS.map((option) => (
            <label key={option.value} className="theme-picker__option">
              <input
                type="radio"
                name="theme"
                value={option.value}
                checked={theme === option.value}
                onChange={() => chooseTheme(option.value)}
              />
              {option.label}
            </label>
          ))}
        </fieldset>
      </section>
      <p className="settings-links">
        <Link to="/diagnostics">Open Diagnostics</Link> for versions, probe results,
        and counters to attach to a bug report.
      </p>
    </RoutePlaceholder>
  );
}
