import { useQuery } from "@tanstack/react-query";

import { getAppInfo } from "../api/client";
import { RoutePlaceholder } from "../components/RoutePlaceholder";

interface InfoRowProps {
  label: string;
  value: string;
  mono?: boolean;
}

function InfoRow({ label, value, mono = false }: InfoRowProps) {
  return (
    <div>
      <dt>{label}</dt>
      <dd className={mono ? "code" : undefined}>{value}</dd>
    </div>
  );
}

export function Settings() {
  const appInfo = useQuery({
    queryKey: ["app-info"],
    queryFn: getAppInfo,
    staleTime: 60_000,
  });

  return (
    <RoutePlaceholder
      title="Settings"
      lead="Where Morphix keeps its data, and which preferences you control."
      note="P7-02 and P8-11: storage locations, theme override, and the rest of preferences."
    >
      <section aria-labelledby="app-info-title">
        <h2 id="app-info-title">Application</h2>
        {appInfo.isPending ? (
          <p role="status">Reading application information…</p>
        ) : null}
        {appInfo.isError ? (
          <p role="alert">
            Could not read application information: {appInfo.error.message}
          </p>
        ) : null}
        {appInfo.isSuccess ? (
          <dl>
            <InfoRow label="Version" value={appInfo.data.version} />
            <InfoRow label="Build" value={appInfo.data.build} />
            <InfoRow label="Log file" value={appInfo.data.logPath} mono />
            <InfoRow label="Data directory" value={appInfo.data.dataDir} mono />
            <InfoRow label="State directory" value={appInfo.data.stateDir} mono />
          </dl>
        ) : null}
      </section>
    </RoutePlaceholder>
  );
}
