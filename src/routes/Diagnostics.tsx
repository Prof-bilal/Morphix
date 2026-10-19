import { RoutePlaceholder } from "../components/RoutePlaceholder";

export function Diagnostics() {
  return (
    <RoutePlaceholder
      title="Diagnostics"
      lead="Local counters, versions, adapter probe results, and log excerpts to attach to a report."
      note="P7-02 and P8-11: diagnostics view and explicit local export."
    />
  );
}
