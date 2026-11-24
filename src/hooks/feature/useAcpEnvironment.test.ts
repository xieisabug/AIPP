import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { useAcpEnvironment } from "./useAcpEnvironment";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: async () => () => {} }));

describe("useAcpEnvironment", () => {
    beforeEach(() => { invoke.mockReset(); });

    it("should use the configured path and ignore an older probe after the path changes", async () => {
        let finishFirst!: (value: unknown) => void;
        invoke.mockImplementation((_cmd, { configuredCommand }) => {
            if (configuredCommand === "claude") return new Promise(resolve => { finishFirst = resolve; });
            return Promise.resolve({ installed: true, installed_path: configuredCommand, cli_command: "claude" });
        });
        const settings = { acp_env_vars: "PATH=C:/Local Agent" };
        const { result, rerender } = renderHook(({ path }) => useAcpEnvironment("claude", path, settings),
            { initialProps: { path: "claude" } });
        await waitFor(() => expect(finishFirst).toBeDefined());
        const path = "C:/Program Files/Claude/claude.exe";
        rerender({ path });
        await waitFor(() => expect(result.current.libraryInfo?.installed_path).toBe(path));
        await act(async () => finishFirst({ installed: false, requires_external_install: true }));
        expect(result.current.status).toBe("installed");
        expect(invoke).toHaveBeenCalledWith("check_acp_library", { cliCommand: "claude", configuredCommand: path, settings });
    });

    it("should check Bun only when the user chooses to install missing Codex", async () => {
        invoke.mockImplementation(async cmd => cmd === "check_acp_library"
            ? { installed: false, detection_error: "找不到命令 codex" } : "Not Installed");
        const { result } = renderHook(() => useAcpEnvironment("codex", "codex"));
        await waitFor(() => expect(result.current.status).toBe("not-installed"));
        expect(invoke).not.toHaveBeenCalledWith("check_bun_version");
        await act(async () => { await result.current.installAcpLibrary(); });
        expect(result.current.status).toBe("bun-not-installed");
    });

    it("should verify updates against the latest version for the selected CLI", async () => {
        const path = "C:/Selected Tools/codex.cmd";
        invoke.mockImplementation(async cmd => cmd === "check_acp_library_update" ? "1.2.3"
            : { installed: true, installed_path: path, cli_command: "codex", version: "1.2.2" });
        const { result } = renderHook(() => useAcpEnvironment("codex", path));
        await waitFor(() => expect(result.current.status).toBe("installed"));
        await act(async () => { await result.current.checkAcpLibraryUpdate(); });
        await act(async () => { await result.current.updateAcpLibrary(); });
        expect(invoke).toHaveBeenCalledWith("update_acp_library", expect.objectContaining({
            cliCommand: "codex", configuredCommand: path, expectedVersion: "1.2.3",
        }));
        expect(result.current.libraryInfo?.version).toBe("1.2.2");
    });
});
