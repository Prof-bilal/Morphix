import { useCallback, useEffect, useRef, useState } from "react";
import { Link } from "react-router-dom";

import {
  getSystemStatus,
  type PackageManager,
  type SessionType,
  type SupportLevel,
  type SupportReason,
  type SystemStatus,
} from "../api/client";

/** Loader seam for tests; the app uses the IPC client. Must be referentially stable. */
export type StatusLoader = (refresh: boolean) => Promise<SystemStatus>;

interface SystemStatusCardProps {
  loadStatus?: StatusLoader;
}

const LEVEL_LABELS: Record<SupportLevel, string> = {
  supported: "Supported",
  unverified: "Unverified",
  unsupported: "Unsupported",
};

const SESSION_LABELS: Record<SessionType, string> = {
  wayland: "Wayland",
  x11: "X11",
  tty: "TTY",
  unknown: "Unknown",
};

const PACKAGE_MANAGER_LABELS: Record<PackageManager, string> = {
  pacman: "pacman",
  dpkg: "dpkg",
  rpm: "rpm",
  zypper: "zypper",
  unknown: "Unknown",
};

/**
 * Presentation copy for the closed reason set. The backend summary is shown as well; these
 * labels make each machine-readable reason understandable without hiding the code.
 */
const REASON_LABELS: Record<SupportReason, string> = {
  NOT_LINUX: "This is not a Linux system.",
  RUNNING_AS_ROOT: "Morphix is running as root.",
  NO_SESSION_BUS: "The Plasma session bus is not reachable.",
  NO_PLASMA: "Plasma is not running in this session.",
  PLASMA_VERSION_UNSUPPORTED: "The Plasma version is outside the supported 6.x range.",
  PLASMA_VERSION_UNKNOWN: "The Plasma version could not be determined.",
  KWIN_UNAVAILABLE: "The KWin compositor is not reachable.",
  KGLOBALACCEL_UNAVAILABLE: "The global shortcut service is not reachable.",
  KDEGLOBALS_UNREADABLE: "The KDE configuration file is not readable.",
  KWINRC_UNREADABLE: "The KWin configuration file is not readable.",
  DATA_DIR_UNWRITABLE: "The Morphix data directory is not writable.",
  PLASMA_MINOR_UNVALIDATED: "This Plasma minor has not been validated.",
  SESSION_TYPE_UNVALIDATED: "This session type has not been validated.",
  ARCHITECTURE_UNVALIDATED: "This architecture has not been validated.",
  MULTI_SCREEN_UNVALIDATED: "Multi-screen setups have not been validated.",
};

/**
 * Dashboard support verdict (UI_SPEC.md §5.1, MVP.md AC-04).
 *
 * Fetches on mount with `refresh: false` and re-detects with `refresh: true` when
 * **Re-check** is pressed. The card never turns a failed call into a verdict: errors are
 * shown as errors, and a root refusal is visually distinct from a generic unsupported
 * system.
 */
export function SystemStatusCard({ loadStatus = getSystemStatus }: SystemStatusCardProps) {
  const [status, setStatus] = useState<SystemStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(true);
  const latestRequest = useRef(0);

  const load = useCallback(
    async (refresh: boolean) => {
      const request = latestRequest.current + 1;
      latestRequest.current = request;
      setLoading(true);
      setError(null);

      try {
        const next = await loadStatus(refresh);
        if (request === latestRequest.current) {
          setStatus(next);
        }
      } catch (cause) {
        if (request === latestRequest.current) {
          setError(cause instanceof Error ? cause.message : String(cause));
        }
      } finally {
        if (request === latestRequest.current) {
          setLoading(false);
        }
      }
    },
    [loadStatus],
  );

  useEffect(() => {
    void load(false);
  }, [load]);

  return (
    <section
      className="status-card"
      aria-labelledby="system-status-heading"
      aria-busy={loading}
    >
      <div className="status-card__header">
        <h2 id="system-status-heading">System status</h2>
        <button
          type="button"
          className="status-card__button"
          onClick={() => void load(true)}
          disabled={loading}
        >
          Re-check
        </button>
      </div>

      {loading && status === null ? (
        <div className="status-card__loading" role="status">
          <span className="status-card__skeleton" />
          <span className="status-card__skeleton status-card__skeleton--short" />
          <span className="sr-only">Checking this system…</span>
        </div>
      ) : null}

      {error !== null ? (
        <div className="status-banner status-banner--error" role="alert">
          <strong>Could not read the system status.</strong>
          <p>{error}</p>
          <p>Nothing was changed. Re-check tries again.</p>
        </div>
      ) : null}

      {status !== null && error === null ? <StatusBody status={status} /> : null}
    </section>
  );
}

function StatusBody({ status }: { status: SystemStatus }) {
  const { support } = status;
  const root = support.reasons.includes("RUNNING_AS_ROOT");
  const level = support.level;

  return (
    <>
      <div className="status-card__verdict">
        <span className={`status-badge status-badge--${level}`}>{LEVEL_LABELS[level]}</span>
        <p>{support.summary}</p>
      </div>

      {root ? (
        <div className="status-banner status-banner--root" role="alert">
          <strong>Morphix refuses to change settings while running as root.</strong>
          <p>Browsing remains available; no customization action can run.</p>
        </div>
      ) : null}

      {level === "unsupported" && !root ? (
        <div className="status-banner status-banner--unsupported">
          <strong>Customization is disabled on this system.</strong>
          <p>The catalog stays readable, but Morphix will not offer an apply action.</p>
          <Link to="/discover">Browse the catalog</Link>
        </div>
      ) : null}

      {level === "unverified" ? (
        <p className="status-card__note">
          You can continue, but some features may behave differently than on the validated
          combination.
        </p>
      ) : null}

      {support.reasons.length > 0 ? (
        <ul className="status-card__reasons" aria-label="Support reasons">
          {support.reasons.map((reason) => (
            <li key={reason}>
              <code>{reason}</code> {REASON_LABELS[reason]}
            </li>
          ))}
        </ul>
      ) : null}

      {level === "supported" ? (
        <details className="status-card__why">
          <summary>Why this is supported</summary>
          <StatusFacts status={status} />
        </details>
      ) : (
        <StatusFacts status={status} />
      )}
    </>
  );
}

function StatusFacts({ status }: { status: SystemStatus }) {
  return (
    <dl className="status-facts">
      <dt>Distribution</dt>
      <dd>{distributionLabel(status)}</dd>
      <dt>Plasma</dt>
      <dd>{status.plasmaVersion === null ? "Not detected" : status.plasmaVersion}</dd>
      <dt>Session</dt>
      <dd>{SESSION_LABELS[status.sessionType]}</dd>
      <dt>Plasma shell</dt>
      <dd>{status.plasmashellRunning ? "Running" : "Not running"}</dd>
      <dt>Package manager</dt>
      <dd>{PACKAGE_MANAGER_LABELS[status.packageManager]}</dd>
    </dl>
  );
}

function distributionLabel(status: SystemStatus): string {
  const { distro } = status;
  return distro.prettyName ?? distro.name ?? distro.id ?? "Unknown distribution";
}
