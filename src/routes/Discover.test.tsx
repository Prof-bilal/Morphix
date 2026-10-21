import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { listPacks, type PackSummary } from "../api/client";
import { Discover } from "./Discover";

vi.mock("../api/client", () => ({
  listPacks: vi.fn(),
}));

const listPacksMock = vi.mocked(listPacks);

const BUNDLED: PackSummary = {
  id: "dev.morphix.cupertino",
  name: "Cupertino",
  version: "1.0.0",
  summary: "A calm light pack.",
  source: "bundled",
  status: "valid",
  compatibility: "Plasma 6.0 - 6.8",
  capabilities: ["appearance.color_scheme", "appearance.wallpaper"],
  applied: false,
  integritySource: "declared",
  issueCount: 0,
};

const INVALID_LOCAL: PackSummary = {
  id: "dev.example.broken",
  name: "Broken",
  summary: "Fails validation.",
  source: "local",
  status: "invalid",
  capabilities: ["appearance.wallpaper"],
  applied: false,
  integritySource: "generated_local",
  issueCount: 2,
};

function renderDiscover() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <MemoryRouter>
      <QueryClientProvider client={client}>
        <Discover />
      </QueryClientProvider>
    </MemoryRouter>,
  );
}

describe("Discover", () => {
  beforeEach(() => {
    listPacksMock.mockReset();
  });

  it("renders pack cards with badges and links to detail", async () => {
    listPacksMock.mockResolvedValue([BUNDLED, INVALID_LOCAL]);
    renderDiscover();

    expect(await screen.findByText("Cupertino")).toBeInTheDocument();
    expect(screen.getByText("bundled")).toBeInTheDocument();
    expect(screen.getByText("local")).toBeInTheDocument();
    expect(screen.getByText("2 capabilities")).toBeInTheDocument();
    expect(screen.getByText(/invalid · 2 issues/)).toBeInTheDocument();

    const links = screen.getAllByRole("link", { name: /View (details|issues) for/ });
    expect(links).toHaveLength(2);
    expect(links[0]).toHaveAttribute("href", `/packs/${BUNDLED.id}`);
  });

  it("shows the empty state with a Diagnostics link", async () => {
    listPacksMock.mockResolvedValue([]);
    renderDiscover();

    expect(await screen.findByText("No packs found.")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Open Diagnostics" })).toHaveAttribute(
      "href",
      "/diagnostics",
    );
  });

  it("shows a failure state without claiming changes", async () => {
    listPacksMock.mockRejectedValue(new Error("list_packs failed: boom"));
    renderDiscover();

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Could not load packs: list_packs failed: boom",
    );
  });
});
