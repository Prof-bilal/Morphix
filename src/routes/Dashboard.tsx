import { SystemStatusCard } from "../components/SystemStatusCard";

export function Dashboard() {
  return (
    <section className="page" aria-labelledby="page-title">
      <h1 className="page__title" id="page-title">
        Dashboard
      </h1>
      <p className="page__lead">
        Whether this system is supported, and what Morphix changed recently.
      </p>
      <SystemStatusCard />
      <p className="page__note">Planned: P8-02 adds the recent-transactions list.</p>
    </section>
  );
}
