import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter } from "react-router-dom";
import { describe, expect, it, vi } from "vitest";

import type {
  SupportLevel,
  SupportReason,
  SystemStatus,
} from "../api/client";
import { SystemStatusCard, type StatusLoader } from "./SystemStatusCard";

function makeStatus(
  level: SupportLevel,
  reasons: SupportReason[],
  overrides: Partial<SystemStatus> = {},
): SystemStatus {
  const summary =
    level === "supported"
      ? "This system meets Morphix's support requirements."
      : level === "unverified"
        ? "Morphix has not validated this system combination."
        : "Morphix cannot run on this system.";

  return {
    supported: level === "supported",
    support: { level, reasons, summary },
    distro: {
      id: "arch",
      name: "Arch Linux",
      prettyName: "Arch Linux",
      versionId: "rolling",
      idLike: ["arch"],
    },
    packageManager: "pacman",
    plasmaVersion: "6.8.0",
    plasmashellRunning: true,
    sessionType: "wayland",
    ...overrides,
  };
}

function renderCard(loadStatus: StatusLoader) {
  return render(
    <MemoryRouter>
      <SystemStatusCard loadStatus={loadStatus} />
    </MemoryRouter>,
  );
}

describe("SystemStatusCard", () => {
  it("shows a supported verdict with the detected facts collapsed under its reason", async () => {
    const loadStatus = vi.fn<StatusLoader>().mockResolvedValue(makeStatus("supported", []));
    renderCard(loadStatus);

    expect(
      await screen.findByRole("heading", { level: 2, name: "System status" }),
    ).toBeInTheDocument();
    expect(await screen.findByText("Supported")).toBeInTheDocument();
    expect(screen.getByText("Why this is supported")).toBeInTheDocument();
    expect(screen.getByText("Arch Linux")).toBeInTheDocument();
    expect(screen.getByText("6.8.0")).toBeInTheDocument();
    expect(screen.getByText("Wayland")).toBeInTheDocument();
    expect(screen.getByText("Running")).toBeInTheDocument();
    expect(screen.queryByRole("list", { name: "Support reasons" })).not.toBeInTheDocument();
    expect(loadStatus).toHaveBeenCalledExactlyOnceWith(false);
  });

  it("shows each refusal reason and keeps the catalog reachable", async () => {
    const loadStatus = vi
      .fn<StatusLoader>()
      .mockResolvedValue(makeStatus("unsupported", ["NO_PLASMA"]));
    renderCard(loadStatus);

    expect(await screen.findByText("Unsupported")).toBeInTheDocument();
    expect(screen.getByText("NO_PLASMA")).toBeInTheDocument();
    expect(screen.getByText("Plasma is not running in this session.")).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Browse the catalog" })).toHaveAttribute(
      "href",
      "/discover",
    );
  });

  it("gives root its own refusal banner and warning copy", async () => {
    const loadStatus = vi
      .fn<StatusLoader>()
      .mockResolvedValue(makeStatus("unsupported", ["RUNNING_AS_ROOT"]));
    renderCard(loadStatus);

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent(
      "Morphix refuses to change settings while running as root.",
    );
    expect(screen.getByText("RUNNING_AS_ROOT")).toBeInTheDocument();
    expect(
      screen.queryByRole("link", { name: "Browse the catalog" }),
    ).not.toBeInTheDocument();
  });

  it("shows an unverified combination without claiming support", async () => {
    const loadStatus = vi
      .fn<StatusLoader>()
      .mockResolvedValue(makeStatus("unverified", ["SESSION_TYPE_UNVALIDATED"]));
    renderCard(loadStatus);

    expect(await screen.findByText("Unverified")).toBeInTheDocument();
    expect(screen.getByText("SESSION_TYPE_UNVALIDATED")).toBeInTheDocument();
    expect(
      screen.getByText(/You can continue, but some features may behave differently/),
    ).toBeInTheDocument();
    expect(screen.getByText("Arch Linux")).toBeInTheDocument();
  });

  it("shows a load error, then re-checks with refresh true", async () => {
    const user = userEvent.setup();
    const loadStatus = vi
      .fn<StatusLoader>()
      .mockRejectedValueOnce(new Error("no session bus"))
      .mockResolvedValueOnce(makeStatus("supported", []));
    renderCard(loadStatus);

    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("no session bus");
    expect(alert).toHaveTextContent("Nothing was changed.");

    await user.click(screen.getByRole("button", { name: "Re-check" }));

    expect(await screen.findByText("Supported")).toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(loadStatus.mock.calls).toEqual([[false], [true]]);
  });

  it("renders a loading state and disables Re-check until it resolves", () => {
    const loadStatus: StatusLoader = () => new Promise<SystemStatus>(() => {});
    renderCard(loadStatus);

    expect(screen.getByText("Checking this system…")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Re-check" })).toBeDisabled();
  });
});
