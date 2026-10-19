import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { getAppInfo } from "../api/client";
import { Settings } from "./Settings";

vi.mock("../api/client", () => ({
  getAppInfo: vi.fn(),
}));

const getAppInfoMock = vi.mocked(getAppInfo);

function renderSettings() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });

  return render(
    <QueryClientProvider client={client}>
      <Settings />
    </QueryClientProvider>,
  );
}

describe("Settings", () => {
  beforeEach(() => {
    getAppInfoMock.mockReset();
  });

  it("renders the application information returned by the shell", async () => {
    getAppInfoMock.mockResolvedValue({
      version: "0.1.0",
      build: "development",
      logPath: "/state/morphix/logs/morphix.log",
      dataDir: "/data/morphix",
      stateDir: "/state/morphix",
    });

    renderSettings();

    expect(await screen.findByText("0.1.0")).toBeInTheDocument();
    expect(screen.getByText("/data/morphix")).toBeInTheDocument();
    expect(screen.getByText("/state/morphix")).toBeInTheDocument();
  });

  it("shows a failure state instead of pretending the app info loaded", async () => {
    getAppInfoMock.mockRejectedValue(new Error("get_app_info failed: no ipc bridge"));

    renderSettings();

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Could not read application information: get_app_info failed: no ipc bridge",
    );
  });
});
