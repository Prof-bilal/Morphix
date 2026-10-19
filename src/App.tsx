import { Navigate, Route, Routes } from "react-router-dom";

import { AppNav } from "./components/AppNav";
import { Dashboard } from "./routes/Dashboard";
import { Diagnostics } from "./routes/Diagnostics";
import { Discover } from "./routes/Discover";
import { PackDetail } from "./routes/PackDetail";
import { Preview } from "./routes/Preview";
import { Settings } from "./routes/Settings";
import { Snapshots } from "./routes/Snapshots";
import { Transactions } from "./routes/Transactions";

/**
 * Application shell and the eight routes from UI_SPEC.md §3.
 *
 * Providers (router, query client) are installed in `main.tsx`, so this component stays
 * renderable in tests without a running desktop or IPC.
 */
export function App() {
  return (
    <div className="app-shell">
      <a className="skip-link" href="#main-content">
        Skip to content
      </a>
      <AppNav />
      <main className="app-main" id="main-content">
        <Routes>
          <Route path="/" element={<Dashboard />} />
          <Route path="/discover" element={<Discover />} />
          <Route path="/packs/:packId" element={<PackDetail />} />
          <Route path="/packs/:packId/preview" element={<Preview />} />
          <Route path="/transactions" element={<Transactions />} />
          <Route path="/snapshots" element={<Snapshots />} />
          <Route path="/settings" element={<Settings />} />
          <Route path="/diagnostics" element={<Diagnostics />} />
          <Route path="*" element={<Navigate to="/" replace />} />
        </Routes>
      </main>
    </div>
  );
}
