import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  getAppInfo,
  getPackDetail,
  getSystemStatus,
  installLocalPack,
  IpcError,
  listPacks,
  type SystemStatus,
} from "./client";

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

describe("pack catalog and install", () => {
  beforeEach(() => {
    invokeMock.mockReset();
  });

  it("lists packs through the documented command", async () => {
    invokeMock.mockResolvedValue([]);

    await expect(listPacks()).resolves.toEqual([]);
    expect(invokeMock).toHaveBeenCalledExactlyOnceWith("list_packs");
  });

  it("requests one pack detail by its camelCase argument", async () => {
    const detail = { summary: { id: "dev.example.one" } };
    invokeMock.mockResolvedValue(detail);

    await expect(getPackDetail("dev.example.one")).resolves.toEqual(detail);
    expect(invokeMock).toHaveBeenCalledExactlyOnceWith("get_pack_detail", {
      packId: "dev.example.one",
    });
  });

  it("installs only through the path command", async () => {
    invokeMock.mockResolvedValue({ id: "dev.example.one" });

    await installLocalPack("/tmp/pack");
    expect(invokeMock).toHaveBeenCalledExactlyOnceWith("install_local_pack", {
      directory: "/tmp/pack",
    });
  });

  it("keeps the validation code and issue detail from a rejected install", async () => {
    invokeMock.mockRejectedValue({
      code: "E_REQUIRED_FILE_MISSING",
      message: "the pack failed validation; nothing was installed",
      detail: {
        issues: [{ code: "E_REQUIRED_FILE_MISSING", path: "README.md" }],
      },
    });

    const error = await installLocalPack("/tmp/pack").catch((failure: unknown) => failure);
    expect(error).toBeInstanceOf(IpcError);
    expect(error).toMatchObject({
      name: "IpcError",
      command: "install_local_pack",
      code: "E_REQUIRED_FILE_MISSING",
      detail: {
        issues: [{ code: "E_REQUIRED_FILE_MISSING", path: "README.md" }],
      },
    });
  });
});
