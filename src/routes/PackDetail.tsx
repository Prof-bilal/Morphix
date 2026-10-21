import { useQuery } from "@tanstack/react-query";
import { Link, useParams } from "react-router-dom";

import { getPackDetail } from "../api/client";

/**
 * Pack detail (UI_SPEC.md §5.3, P2-12/P8-04): description → declared changes →
 * assets → compatibility → license/attribution → previews → actions, in that
 * fixed order. Invalid packs show field-level errors read-only. Unknown ids get
 * a plain not-found with a Diagnostics link. Apply/preview actions belong to
 * Phase 8 and are absent, not disabled.
 */
export function PackDetail() {
  const { packId } = useParams();
  const detail = useQuery({
    queryKey: ["pack-detail", packId],
    queryFn: () => getPackDetail(packId ?? ""),
    enabled: packId !== undefined && packId !== "",
    staleTime: 30_000,
  });

  return (
    <section className="page" aria-labelledby="page-title">
      <h1 className="page__title" id="page-title">
        Pack Detail
      </h1>
      {(packId === undefined || packId === "") && (
        <p role="alert">
          No pack selected. <Link to="/discover">Back to Discover</Link>
        </p>
      )}
      {detail.isPending ? <p role="status">Loading pack…</p> : null}
      {detail.isError ? (
        <p role="alert">
          {detail.error.message.includes("E_INVALID_ID") ||
          detail.error.message.includes("no available pack") ? (
            <>
              Pack not found. <Link to="/discover">Back to Discover</Link> or{" "}
              <Link to="/diagnostics">open Diagnostics</Link>.
            </>
          ) : (
            <>Could not load pack: {detail.error.message}</>
          )}
        </p>
      ) : null}
      {detail.isSuccess ? (
        <article aria-labelledby="pack-detail-name">
          <h2 id="pack-detail-name">{detail.data.summary.name}</h2>
          <p className="pack-detail__summary">{detail.data.summary.summary}</p>
          <ul
            className="pack-card__badges"
            aria-label="Pack properties"
          >
            <li
              className={
                detail.data.summary.source === "bundled"
                  ? "badge badge--bundled"
                  : "badge badge--local"
              }
            >
              {detail.data.summary.source}
            </li>
            {detail.data.summary.compatibility ? (
              <li className="badge">{detail.data.summary.compatibility}</li>
            ) : null}
            {detail.data.summary.status === "invalid" ? (
              <li className="badge badge--invalid">invalid</li>
            ) : null}
            {detail.data.summary.applied ? (
              <li className="badge badge--applied">applied</li>
            ) : null}
          </ul>
          <p>{detail.data.description}</p>
          {detail.data.summary.status === "invalid" &&
          detail.data.issues.length > 0 ? (
            <section aria-labelledby="pack-issues-title">
              <h3 id="pack-issues-title">Validation issues</h3>
              <p>
                This pack cannot be used until these are fixed. Nothing was
                installed.
              </p>
              <ul className="issue-list">
                {detail.data.issues.map((issue, index) => (
                  <li key={`${issue.code}-${index}`}>
                    <span className="code">{issue.code}</span> — {issue.message}
                    {issue.path ? (
                      <>
                        {" "}
                        (<span className="code">{issue.path}</span>)
                      </>
                    ) : null}
                  </li>
                ))}
              </ul>
            </section>
          ) : null}
          {detail.data.declaredChanges.length > 0 ? (
            <section aria-labelledby="pack-changes-title">
              <h3 id="pack-changes-title">What will change</h3>
              <ul className="change-list">
                {detail.data.declaredChanges.map((change, index) => (
                  <li key={`${change.capability}-${change.target}-${index}`}>
                    <span className="code">{change.capability}</span> ·{" "}
                    <span className="code">{change.kind}</span> ·{" "}
                    <span className="code">{change.target}</span>
                  </li>
                ))}
              </ul>
            </section>
          ) : null}
          {detail.data.assets.length > 0 ? (
            <section aria-labelledby="pack-assets-title">
              <h3 id="pack-assets-title">Assets</h3>
              <ul className="asset-list">
                {detail.data.assets.map((asset) => (
                  <li key={`${asset.kind}-${asset.key}`}>
                    {asset.key}{" "}
                    <span className="code">
                      ({asset.kind}
                      {asset.isDirectory ? ", directory" : ""},{" "}
                      {asset.bytes} bytes)
                    </span>
                  </li>
                ))}
              </ul>
            </section>
          ) : null}
          {detail.data.dependencies.length > 0 ? (
            <section aria-labelledby="pack-deps-title">
              <h3 id="pack-deps-title">Dependencies</h3>
              <ul className="asset-list">
                {detail.data.dependencies.map((dependency) => (
                  <li key={dependency.id}>
                    <span className="code">{dependency.id}</span> — v
                    {dependency.version}
                  </li>
                ))}
              </ul>
            </section>
          ) : null}
          <section aria-labelledby="pack-meta-title">
            <h3 id="pack-meta-title">License and attribution</h3>
            <p>
              {detail.data.license}
              {detail.data.authors.length > 0
                ? ` · by ${detail.data.authors.join(", ")}`
                : ""}
            </p>
            {detail.data.homepage ? (
              <p className="code">{detail.data.homepage}</p>
            ) : null}
            {detail.data.attribution ? (
              <p>{detail.data.attribution}</p>
            ) : null}
          </section>
        </article>
      ) : null}
    </section>
  );
}
