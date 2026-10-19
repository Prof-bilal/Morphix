import { useParams } from "react-router-dom";

import { RoutePlaceholder } from "../components/RoutePlaceholder";

export function PackDetail() {
  const { packId } = useParams();

  return (
    <RoutePlaceholder
      title="Pack Detail"
      lead="What exactly this pack does: declared change set, capabilities, license, and previews."
      note="P2-12 and P8-04: pack detail, asset inventory, attribution, and compatibility."
    >
      <p className="page__lead">
        Pack: <span className="code">{packId ?? "unknown"}</span>
      </p>
    </RoutePlaceholder>
  );
}
