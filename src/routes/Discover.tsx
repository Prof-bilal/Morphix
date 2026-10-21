import { useQuery } from "@tanstack/react-query";
import { Link } from "react-router-dom";

import { listPacks } from "../api/client";
import { PackCard } from "../components/PackCard";

/**
 * Pack catalog (UI_SPEC.md §5.2, P2-12): cards with name, summary, capability
 * count, source and compatibility badges, invalid packs surfaced with a Details
 * link, and an empty state that points at Diagnostics when the store is bare.
 *
 * Installing a local pack needs the OS directory picker through Rust
 * (`install_local_pack` + a dialog capability, P8-03). That picker does not
 * exist yet, so no install button is shown: a control that cannot work must be
 * absent, not disabled (AGENTS.md §12 analogue for desktop UI).
 */
export function Discover() {
  const packs = useQuery({
    queryKey: ["packs"],
    queryFn: listPacks,
    staleTime: 30_000,
  });

  return (
    <section className="page" aria-labelledby="page-title">
      <h1 className="page__title" id="page-title">
        Discover
      </h1>
      <p className="page__lead">
        The catalog of customization packs available locally, with compatibility
        and source.
      </p>
      {packs.isPending ? (
        <div role="status" aria-label="Loading packs">
          <div className="pack-grid" aria-hidden="true">
            <div className="pack-card pack-card--skeleton" />
            <div className="pack-card pack-card--skeleton" />
            <div className="pack-card pack-card--skeleton" />
          </div>
        </div>
      ) : null}
      {packs.isError ? (
        <p role="alert">
          Could not load packs: {packs.error.message} Nothing was changed.
        </p>
      ) : null}
      {packs.isSuccess && packs.data.length === 0 ? (
        <div className="empty-state">
          <p>No packs found.</p>
          <p>
            Morphix ships bundled packs; if none appear, the pack store could
            not be read.
          </p>
          <p>
            <Link to="/diagnostics">Open Diagnostics</Link> to check what the
            pack store reported.
          </p>
        </div>
      ) : null}
      {packs.isSuccess && packs.data.length > 0 ? (
        <div className="pack-grid">
          {packs.data.map((pack) => (
            <PackCard key={pack.id} pack={pack} />
          ))}
        </div>
      ) : null}
    </section>
  );
}
