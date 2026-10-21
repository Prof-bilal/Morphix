import { Link } from "react-router-dom";

import type { PackSummary } from "../api/client";

/**
 * One catalog card (UI_SPEC.md §5.2): name, summary, capability count, source
 * badge, compatibility badge, and an `invalid` badge that links to the detail
 * view instead of hiding the pack.
 */
export function PackCard({ pack }: { pack: PackSummary }) {
  return (
    <article className="pack-card" aria-labelledby={`pack-${pack.id}`}>
      <h3 id={`pack-${pack.id}`} className="pack-card__name">
        <Link to={`/packs/${pack.id}`}>{pack.name}</Link>
      </h3>
      <p className="pack-card__summary">{pack.summary}</p>
      <ul className="pack-card__badges" aria-label="Pack properties">
        <li
          className={
            pack.source === "bundled"
              ? "badge badge--bundled"
              : "badge badge--local"
          }
        >
          {pack.source}
        </li>
        {pack.compatibility ? (
          <li className="badge">{pack.compatibility}</li>
        ) : null}
        <li className="badge">
          {pack.capabilities.length === 1
            ? "1 capability"
            : `${pack.capabilities.length} capabilities`}
        </li>
        {pack.applied ? <li className="badge badge--applied">applied</li> : null}
        {pack.status === "invalid" ? (
          <li className="badge badge--invalid">
            invalid · {pack.issueCount}{" "}
            {pack.issueCount === 1 ? "issue" : "issues"}
          </li>
        ) : null}
      </ul>
      <p className="pack-card__actions">
        <Link
          to={`/packs/${pack.id}`}
          aria-label={
            pack.status === "invalid"
              ? `View issues for ${pack.name}`
              : `View details for ${pack.name}`
          }
        >
          {pack.status === "invalid" ? "Details" : "Details"}
        </Link>
      </p>
    </article>
  );
}
