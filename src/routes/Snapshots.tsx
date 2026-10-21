import { Link } from "react-router-dom";

/**
 * Snapshots (P8-08 owns the list, detail, and restore preview). The Phase 4
 * engine exists and is tested, but no IPC exposes snapshots yet — snapshot
 * listing and restore planning are wired through the Phase 5 apply path. This
 * screen states what the engine guarantees instead of a fake list.
 */
export function Snapshots() {
  return (
    <section className="page" aria-labelledby="page-title">
      <h1 className="page__title" id="page-title">
        Snapshots
      </h1>
      <p className="page__lead">
        The states Morphix recorded before it changed anything, and how to
        return to them.
      </p>
      <div className="empty-state">
        <p>No snapshots yet.</p>
        <p>
          Every applied change is captured first — whole config files, per-key
          values, asset trees, wallpaper descriptors — and verified before the
          change runs. When snapshots exist, each one appears here with exactly
          what changed since and what restoring it would do.
        </p>
        <p>
          <Link to="/discover">Browse the catalog</Link> to see what can be
          applied.
        </p>
      </div>
    </section>
  );
}
