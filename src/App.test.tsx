import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router-dom";
import { describe, expect, it } from "vitest";

import { App } from "./App";

function renderShell(initialPath = "/") {
  return render(
    <MemoryRouter initialEntries={[initialPath]}>
      <App />
    </MemoryRouter>,
  );
}

describe("App shell", () => {
  it("renders the five primary destinations and Diagnostics", () => {
    renderShell();

    const nav = screen.getByRole("navigation", { name: "Main" });
    for (const label of [
      "Dashboard",
      "Discover",
      "Transactions",
      "Snapshots",
      "Settings",
      "Diagnostics",
    ]) {
      expect(nav).toHaveTextContent(label);
    }
  });

  it("shows the Dashboard at the root and marks it as the current page", () => {
    renderShell();

    expect(screen.getByRole("heading", { level: 1, name: "Dashboard" })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Dashboard" })).toHaveAttribute(
      "aria-current",
      "page",
    );
  });

  it("navigates to Discover from the rail", async () => {
    const user = userEvent.setup();
    renderShell();

    await user.click(screen.getByRole("link", { name: "Discover" }));

    expect(screen.getByRole("heading", { level: 1, name: "Discover" })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Discover" })).toHaveAttribute(
      "aria-current",
      "page",
    );
  });

  it("renders a pack route with its parameter and a truthful placeholder", () => {
    renderShell("/packs/cupertino/preview");

    expect(screen.getByRole("heading", { level: 1, name: "Preview" })).toBeInTheDocument();
    expect(screen.getByText("cupertino")).toBeInTheDocument();
    expect(screen.getByText(/Planned: P3-11/)).toBeInTheDocument();
  });
});
