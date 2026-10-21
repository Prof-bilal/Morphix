import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { MemoryRouter, Route, Routes } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { getPackDetail, type PackDetail as PackDetailData } from "../api/client";
import { PackDetail } from "./PackDetail";

vi.mock("../api/client", () => ({
  getPackDetail: vi.fn(),
}));

const getPackDetailMock = vi.mocked(getPackDetail);

const DETAIL: PackDetailData = {
  summary: {
    id: "dev.morphix.cupertino",
    name: "Cupertino",
    version: "1.0.0",
    summary: "A calm light pack.",
    source: "bundled",
    status: "valid",
    compatibility: "Plasma 6.0 - 6.8",
    capabilities: ["appearance.color_scheme"],
    applied: false,
    integritySource: "declared",
    issueCount: 0,
  },
  description: "Full description of the pack.",
  authors: ["Morphix"],
  license: "MIT",
  assets: [
    {
      kind: "wallpaper",
      key: "wall",
      path: "wallpapers/wall.png",
      installedName: "cupertino-wall",
      isDirectory: false,
      bytes: 12,
    },
  ],
  declaredChanges: [
    { capability: "appearance.wallpaper", kind: "wallpaper", target: "wall" },
  ],
  previews: [],
  dependencies: [],
  issues: [],
  contentHash: "a".repeat(64),
};

function renderDetail(path = "/packs/dev.morphix.cupertino") {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return render(
    <MemoryRouter initialEntries={[path]}>
      <QueryClientProvider client={client}>
        <Routes>
          <Route path="/packs/:packId" element={<PackDetail />} />
        </Routes>
      </QueryClientProvider>
    </MemoryRouter>,
  );
}

describe("PackDetail", () => {
  beforeEach(() => {
    getPackDetailMock.mockReset();
    getPackDetailMock.mockResolvedValue(DETAIL);
  });

  it("renders the detail sections in spec order", async () => {
    renderDetail();

    expect(await screen.findByText("Cupertino")).toBeInTheDocument();
    expect(screen.getByText("Full description of the pack.")).toBeInTheDocument();
    expect(screen.getByText("What will change")).toBeInTheDocument();
    expect(screen.getByText("Assets")).toBeInTheDocument();
    expect(screen.getByText("License and attribution")).toBeInTheDocument();
  });

  it("surfaces validation issues read-only for invalid packs", async () => {
    getPackDetailMock.mockResolvedValue({
      ...DETAIL,
      summary: { ...DETAIL.summary, status: "invalid" },
      issues: [
        { code: "E_MANIFEST_SYNTAX", message: "bad TOML", path: "pack.toml" },
      ],
    });
    renderDetail();

    expect(await screen.findByText("Validation issues")).toBeInTheDocument();
    expect(screen.getByText(/bad TOML/)).toBeInTheDocument();
  });

  it("shows not-found with navigation for unknown ids", async () => {
    getPackDetailMock.mockRejectedValue(
      new Error("get_pack_detail failed: E_INVALID_ID: no available pack"),
    );
    renderDetail("/packs/nope");

    expect(await screen.findByText(/Pack not found/)).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Back to Discover" })).toHaveAttribute(
      "href",
      "/discover",
    );
  });
});
