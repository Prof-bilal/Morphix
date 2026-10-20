import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { getAppInfo, getSystemStatus, IpcError, type SystemStatus } from "./client";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

const invokeMock = vi.mocked(invoke);

const statusFixture: SystemStatus = {
  supported: true,
  support: {
    level: "supported",
    reasons: [],
    summary: "This system meets Morphix's support requirements.",
  },
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
};

describe("getAppInfo", () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it("calls the documented command and returns its typed payload", async () => {
    const payload = {
      version: "0.1.0",
      build: "development",
      logPath: "/home/tester/.local/state/morphix/logs/morphix.log",
      dataDir: "/home/tester/.local/share/morphix",
      stateDir: "/home/tester/.local/state/morphix",
    };
    invokeMock.mockResolvedValue(payload);

    await expect(getAppInfo()).resolves.toEqual(payload);
    expect(invokeMock).toHaveBeenCalledExactlyOnceWith("get_app_info");
  });

  it("preserves the command name when IPC fails", async () => {
    invokeMock.mockRejectedValue("no ipc bridge");

    await expect(getAppInfo()).rejects.toMatchObject({
      name: "IpcError",
      command: "get_app_info",
      message: "get_app_info failed: no ipc bridge",
    });
    await expect(getAppInfo()).rejects.toBeInstanceOf(IpcError);
  });
});

describe("getSystemStatus", () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it("requests a cached status by default", async () => {
    invokeMock.mockResolvedValue(statusFixture);

    await expect(getSystemStatus()).resolves.toEqual(statusFixture);
    expect(invokeMock).toHaveBeenCalledExactlyOnceWith("get_system_status", {
      refresh: false,
    });
  });

  it("passes refresh true for Re-check", async () => {
    invokeMock.mockResolvedValue(statusFixture);

    await getSystemStatus(true);
    expect(invokeMock).toHaveBeenCalledExactlyOnceWith("get_system_status", {
      refresh: true,
    });
  });

  it("preserves the command name when IPC fails", async () => {
    invokeMock.mockRejectedValue(new Error("no session bus"));

    await expect(getSystemStatus(true)).rejects.toMatchObject({
      name: "IpcError",
      command: "get_system_status",
      message: "get_system_status failed: no session bus",
    });
  });
});
