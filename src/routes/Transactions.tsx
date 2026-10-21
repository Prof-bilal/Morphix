import { Link } from "react-router-dom";

/**
 * Transactions (P5-19/P8-07 own the real list, rollback dialog, and conflicts).
 * No transaction IPC exists yet — the Phase 5 engine is not started — so this
 * screen states that plainly instead of rendering a fake list or a placeholder
 * ticket number.
 */
export function Transactions() {
  return (
    <section className="page" aria-labelledby="page-title">
      <h1 className="page__title" id="page-title">
        Transactions
      </h1>
      <p className="page__lead">
        What Morphix has done, and how to undo it safely.
      </p>
      <div className="empty-state">
        <p>No transactions yet.</p>
        <p>
          Nothing has been applied from this app, so there is nothing to undo.
          When packs are applied, each change appears here with its rollback.
        </p>
        <p>
          <Link to="/discover">Browse the catalog</Link> to see what can be
          applied.
        </p>
      </div>
    </section>
  );
}
