import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
    cleanUpdateLog,
    resetNativeAgentUpdateCache,
    useNativeAgentEnvironment,
    type NativeAgentEnvironment,
} from "./useNativeAgentEnvironment";

const mocks = vi.hoisted(() => ({ invoke: vi.fn(), onmessage: undefined as undefined | ((text: string) => void) }));
vi.mock("@tauri-apps/api/core", () => ({
    invoke: mocks.invoke,
    Channel: class {
        set onmessage(handler: (text: string) => void) { mocks.onmessage = handler; }
    },
}));

function environment(version: string | null, canUpdate = true): NativeAgentEnvironment {
    const program = { path: "C:\\Users\\u\\.pi\\agent\\bin\\pi.ps1", source: "Pi 安装器", version, ok: version !== null, warning: null, error: version ? null : "未找到 pi" };
    return {
        driver: "pi",
        node: { configured: "", required: "22.19.0", selected: { path: "C:\\node\\node.exe", source: "PATH", version: "25.9.0", ok: true, warning: null, error: null }, candidates: [] },
        tool: { kind: "cli", display_name: "Pi", default_command: "pi", configured: "", selected: program, candidates: [program], install_commands: [], docs_url: "", can_update: canUpdate, managed_directory: null, warnings: [], account: null, uses_api_key: false },
        working_directory: { configured: "", resolved: "C:\\Users\\u", exists: true },
    };
}

describe("useNativeAgentEnvironment", () => {
    beforeEach(() => {
        mocks.invoke.mockReset();
        mocks.onmessage = undefined;
        resetNativeAgentUpdateCache();
    });

    it("should detect with the form settings and look for a newer release when a version is installed", async () => {
        mocks.invoke.mockImplementation((command: string) => command === "detect_native_agent_environment"
            ? Promise.resolve(environment("1.5.1"))
            : Promise.resolve({ latest: "1.5.2", update_available: true, source: "npm" }));
        const { result } = renderHook(() => useNativeAgentEnvironment("pi", { agent_cli_command: "" }, 0));
        await waitFor(() => expect(result.current.updateInfo?.latest).toBe("1.5.2"));
        expect(mocks.invoke).toHaveBeenCalledWith("detect_native_agent_environment", { driver: "pi", settings: { agent_cli_command: "" }, providerId: null });
        expect(mocks.invoke).toHaveBeenCalledWith("check_native_agent_update", { driver: "pi", currentVersion: "1.5.1", useProxy: false });
        expect(result.current.currentVersion).toBe("1.5.1");
    });

    it("should not query releases when the program is missing or cannot be updated", async () => {
        mocks.invoke.mockResolvedValue(environment(null));
        const { result } = renderHook(() => useNativeAgentEnvironment("pi", {}, 0));
        await waitFor(() => expect(result.current.environment).not.toBeNull());
        expect(mocks.invoke).not.toHaveBeenCalledWith("check_native_agent_update", expect.anything());
    });

    it("should reuse a cached release lookup when another form shows the same installed version", async () => {
        mocks.invoke.mockImplementation((command: string) => command === "detect_native_agent_environment"
            ? Promise.resolve(environment("1.5.1"))
            : Promise.resolve({ latest: "1.5.1", update_available: false, source: "npm" }));
        const first = renderHook(() => useNativeAgentEnvironment("pi", {}, 0));
        await waitFor(() => expect(first.result.current.updateInfo).not.toBeNull());
        const second = renderHook(() => useNativeAgentEnvironment("pi", {}, 0));
        await waitFor(() => expect(second.result.current.updateInfo?.update_available).toBe(false));
        expect(mocks.invoke.mock.calls.filter(([command]) => command === "check_native_agent_update")).toHaveLength(1);
    });

    it("should keep the direct failure and allow a proxy retry when the release check fails", async () => {
        mocks.invoke.mockImplementation((command: string, args: { useProxy?: boolean }) => command === "detect_native_agent_environment"
            ? Promise.resolve(environment("1.5.1"))
            : args.useProxy ? Promise.resolve({ latest: "1.5.2", update_available: true, source: "npm" }) : Promise.reject("请求 https://registry.npmjs.org 失败：timeout"));
        const { result } = renderHook(() => useNativeAgentEnvironment("pi", {}, 0));
        await waitFor(() => expect(result.current.checkError).toContain("直连检查更新失败"));
        expect(result.current.checkError).toContain("timeout");
        await act(() => result.current.checkUpdate({ useProxy: true }));
        expect(result.current.checkError).toBeNull();
        expect(result.current.updateInfo?.latest).toBe("1.5.2");
    });

    it("should stream updater output and re-detect the new version when the update succeeds", async () => {
        let installed = "1.5.1";
        mocks.invoke.mockImplementation((command: string) => {
            if (command === "detect_native_agent_environment") return Promise.resolve(environment(installed));
            if (command === "check_native_agent_update") return Promise.resolve({ latest: "1.5.2", update_available: installed !== "1.5.2", source: "npm" });
            mocks.onmessage?.("\u001b[32mInstalling 1.5.2\u001b[0m\r50%\r\n");
            installed = "1.5.2";
            return Promise.resolve(null);
        });
        const { result } = renderHook(() => useNativeAgentEnvironment("pi", { agent_cli_command: "pi" }, 0));
        await waitFor(() => expect(result.current.updateInfo?.update_available).toBe(true));
        await act(async () => { await result.current.runUpdate(false); });
        expect(mocks.invoke).toHaveBeenCalledWith("run_native_agent_update", expect.objectContaining({ driver: "pi", settings: { agent_cli_command: "pi" }, useProxy: false }));
        expect(result.current.updateLog).toBe("Installing 1.5.2\n50%\r\n");
        await waitFor(() => expect(result.current.currentVersion).toBe("1.5.2"));
        await waitFor(() => expect(result.current.updateInfo?.update_available).toBe(false));
    });

    it("should surface the updater error with its output when the update fails", async () => {
        mocks.invoke.mockImplementation((command: string) => {
            if (command === "detect_native_agent_environment") return Promise.resolve(environment("1.5.1"));
            if (command === "check_native_agent_update") return Promise.resolve({ latest: "1.5.2", update_available: true, source: "npm" });
            return Promise.reject("Pi 更新失败（pi update self，退出码 1）：EACCES");
        });
        const { result } = renderHook(() => useNativeAgentEnvironment("pi", {}, 0));
        await waitFor(() => expect(result.current.environment).not.toBeNull());
        await act(async () => { await expect(result.current.runUpdate(false)).rejects.toContain("EACCES"); });
        expect(result.current.updateError).toContain("退出码 1");
        expect(result.current.updating).toBe(false);
    });

    it("should stay idle when no native driver is selected", async () => {
        renderHook(() => useNativeAgentEnvironment("", {}, 0));
        await new Promise((resolve) => setTimeout(resolve, 10));
        expect(mocks.invoke).not.toHaveBeenCalled();
    });
});

describe("cleanUpdateLog", () => {
    it("should strip terminal colours and split progress redraws when showing updater output", () => {
        expect(cleanUpdateLog("\u001b[1;32m✓\u001b[0m done\r10%\r20%\r\nnext")).toBe("✓ done\n10%\n20%\r\nnext");
    });
});

describe("useNativeAgentEnvironment provider switching", () => {
    beforeEach(() => { mocks.invoke.mockReset(); resetNativeAgentUpdateCache(); });

    it("should drop the previous provider's results when detection is disabled while another provider loads", async () => {
        mocks.invoke.mockImplementation((command: string) => command === "detect_native_agent_environment"
            ? Promise.resolve(environment("1.5.1"))
            : Promise.resolve({ latest: "1.5.1", update_available: false, source: "npm" }));
        const { result, rerender } = renderHook(({ driver }) => useNativeAgentEnvironment(driver, {}, 0), { initialProps: { driver: "pi" } });
        await waitFor(() => expect(result.current.environment).not.toBeNull());
        rerender({ driver: "" });
        expect(result.current.environment).toBeNull();
        expect(result.current.currentVersion).toBeNull();
    });
});
