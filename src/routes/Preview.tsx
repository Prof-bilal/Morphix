import { useParams } from "react-router-dom";

import { RoutePlaceholder } from "../components/RoutePlaceholder";

export function Preview() {
  const { packId } = useParams();

  return (
    <RoutePlaceholder
      title="Preview"
      lead="The resolved change plan, every old → new value, warnings, and the consent gate."
      note="P3-11 and P8-05: plan rendering, diffs, capability consent, and apply."
    >
      <p className="page__lead">
        Pack: <span className="code">{packId ?? "unknown"}</span>
      </p>
    </RoutePlaceholder>
  );
}
