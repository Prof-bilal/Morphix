/**
 * Required statement on Settings and Diagnostics (UI_SPEC.md §4): the app has no
 * network capability, stated as text rather than a colour alone.
 */
export function OfflineBadge() {
  return (
    <p className="offline-badge">
      <strong>Offline:</strong> Morphix never connects to the network.
    </p>
  );
}
