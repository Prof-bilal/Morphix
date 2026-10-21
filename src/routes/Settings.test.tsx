import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router-dom";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { getAppInfo } from "../api/client";
import { Settings } from "./Settings";

vi.mock("../api/client", () => ({
  getAppInfo: vi.fn(),
}));

const getAppInfoMock = vi.mocked(getAppInfo);

const APP_INFO = {
  version: "0.1.0",
  build: "development",
  logPath: "/state/morphix/logs/morphix.log",
  dataDir: "/data/morphix",
  stateDir: "/state/morphix",
};

function renderSettings() {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });

  return render(
    <MemoryRouter>
      <QueryClientProvider client={client}>
        <Settings />
      </QueryClientProvider>
    </MemoryRouter>,
  );
}

describe("Settings", () => {
  beforeEach(() => {
    getAppInfoMock.mockReset();
    getAppInfoMock.mockResolvedValue({ ...APP_INFO });
    window.localStorage.clear();
    document.documentElement.removeAttribute("data-theme");
  });

  it("renders the application information returned by the shell", async () => {
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

  it("copies a storage path and confirms the copy", async () => {
    const user = userEvent.setup();
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.defineProperty(window.navigator, "clipboard", {
      value: { writeText },
      configurable: true,
    });

    renderSettings();
    await screen.findByText("/state/morphix/logs/morphix.log");

    await user.click(screen.getByRole("button", { name: "Copy log path" }));

    expect(writeText).toHaveBeenCalledWith("/state/morphix/logs/morphix.log");
    expect(
      await screen.findByRole("button", { name: "Copied" }),
    ).toBeInTheDocument();
  });

  it("leaves the copy button unchanged when the clipboard write fails", async () => {
    const user = userEvent.setup();
    Object.defineProperty(window.navigator, "clipboard", {
      value: { writeText: vi.fn().mockRejectedValue(new Error("denied")) },
      configurable: true,
    });

    renderSettings();
    await screen.findByText("/state/morphix/logs/morphix.log");

    await user.click(screen.getByRole("button", { name: "Copy log path" }));

    expect(
      screen.getByRole("button", { name: "Copy log path" }),
    ).toBeInTheDocument();
  });

  it("switches the theme preference and persists the choice", async () => {
    const user = userEvent.setup();
    renderSettings();
    await screen.findByText("0.1.0");

    expect(screen.getByRole("radio", { name: "System" })).toBeChecked();

    await user.click(screen.getByRole("radio", { name: "Dark" }));

    expect(screen.getByRole("radio", { name: "Dark" })).toBeChecked();
    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
    expect(window.localStorage.getItem("morphix.theme")).toBe("dark");

    await user.click(screen.getByRole("radio", { name: "System" }));

    expect(document.documentElement.hasAttribute("data-theme")).toBe(false);
    expect(window.localStorage.getItem("morphix.theme")).toBe("system");
  });

  it("states the offline guarantee and links to Diagnostics", async () => {
    renderSettings();
    await screen.findByText("0.1.0");

    expect(
      screen.getByText("Morphix never connects to the network."),
    ).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Open Diagnostics" })).toHaveAttribute(
      "href",
      "/diagnostics",
    );
  });
});
