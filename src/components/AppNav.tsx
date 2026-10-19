import { NavLink } from "react-router-dom";

/** Top-level destinations, in UI_SPEC.md §3 order. */
const PRIMARY_ROUTES = [
  { to: "/", label: "Dashboard", end: true },
  { to: "/discover", label: "Discover", end: false },
  { to: "/transactions", label: "Transactions", end: false },
  { to: "/snapshots", label: "Snapshots", end: false },
  { to: "/settings", label: "Settings", end: false },
] as const;

/** Persistent left rail: five primary entries plus Diagnostics in the footer. */
export function AppNav() {
  return (
    <nav className="app-nav" aria-label="Main">
      <p className="app-nav__brand">Morphix</p>
      <ul className="app-nav__list">
        {PRIMARY_ROUTES.map((route) => (
          <li key={route.to}>
            <NavLink className="app-nav__link" to={route.to} end={route.end}>
              {route.label}
            </NavLink>
          </li>
        ))}
      </ul>
      <div className="app-nav__footer">
        <NavLink className="app-nav__link" to="/diagnostics">
          Diagnostics
        </NavLink>
      </div>
    </nav>
  );
}
