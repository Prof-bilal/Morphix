import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { getAppInfo, IpcError } from "./client";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
}));

const invokeMock = vi.mocked(invoke);

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
