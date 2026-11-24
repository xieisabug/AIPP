import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { AcpLibraryInfo, useAcpEnvironment } from "@/hooks/feature/useAcpEnvironment";
import { CliEnvironmentStatus, versionNumber } from "./CliEnvironmentStatus";

vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ emit: vi.fn() }));

type Env = ReturnType<typeof useAcpEnvironment>;

const library = (overrides: Partial<AcpLibraryInfo> = {}): AcpLibraryInfo => ({
    cli_command: "codex",
    package_name: "@openai/codex",
    installed: true,
    version: "0.63.0",
    installed_path: "C:\\Users\\u\\AppData\\Roaming\\npm\\codex.cmd",
    requires_external_install: false,
    can_update: true,
    install_hint: "",
    ...overrides,
});

function env(overrides: Partial<Env> = {}): Env {
    return {
        status: "installed",
        libraryInfo: library(),
        installLog: "",
        bunVersion: "1.2.0",
        isCheckingUpdate: false,
        latestVersion: null,
        hasCheckedUpdate: false,
        checkUpdateError: null,
        updateError: null,
        canRetryCheckWithProxy: false,
        canRetryUpdateWithProxy: false,
        checkAcpLibrary: vi.fn(),
        checkAcpLibraryUpdate: vi.fn(),
        installAcpLibrary: vi.fn(),
        updateAcpLibrary: vi.fn(),
        ...overrides,
    } as Env;
}

describe("CliEnvironmentStatus", () => {
    it("should show the installed version and path as a neutral line when the CLI is installed", () => {
        const { container } = render(<CliEnvironmentStatus env={env()} cliCommand="codex" displayName="Codex CLI" />);
        expect(screen.getByText("v0.63.0")).toBeInTheDocument();
        expect(screen.getByText(/codex\.cmd/)).toBeInTheDocument();
        expect(screen.getByRole("button", { name: "检查更新" })).toBeInTheDocument();
        expect(screen.getByRole("button", { name: "重新检测" })).toBeInTheDocument();
        expect(container.innerHTML).not.toMatch(/green|emerald|yellow/);
    });

    it("should show only the version number and offer update checks when Claude Code prints extra text with --version", () => {
        render(
            <CliEnvironmentStatus
                env={env({ libraryInfo: library({ cli_command: "claude", version: "2.1.3 (Claude Code)", requires_external_install: true }) })}
                cliCommand="claude"
                displayName="Claude Code"
            />,
        );
        expect(screen.getByText("v2.1.3")).toBeInTheDocument();
        // 需手动安装但有自带更新命令（claude update），仍可检查更新
        expect(screen.getByRole("button", { name: "检查更新" })).toBeInTheDocument();
    });

    it("should hide update checks when the CLI cannot be updated by AIPP", () => {
        render(
            <CliEnvironmentStatus
                env={env({ libraryInfo: library({ cli_command: "kimi", version: "kimi, version 0.58.0", requires_external_install: true, can_update: false }) })}
                cliCommand="kimi"
                displayName="Kimi Code CLI"
            />,
        );
        expect(screen.getByText("v0.58.0")).toBeInTheDocument();
        expect(screen.queryByRole("button", { name: "检查更新" })).not.toBeInTheDocument();
        expect(screen.getByRole("button", { name: "重新检测" })).toBeInTheDocument();
    });

    it("should offer the update when a newer version was found", () => {
        const updateAcpLibrary = vi.fn();
        render(<CliEnvironmentStatus env={env({ latestVersion: "0.64.0", hasCheckedUpdate: true, updateAcpLibrary })} cliCommand="codex" displayName="Codex CLI" />);
        expect(screen.getByText(/新版本 v0.64.0 可用/)).toBeInTheDocument();
        fireEvent.click(screen.getByRole("button", { name: "更新" }));
        expect(updateAcpLibrary).toHaveBeenCalledWith(false);
    });

    it("should keep showing the installed CLI when re-detecting", () => {
        render(<CliEnvironmentStatus env={env({ status: "checking" })} cliCommand="codex" displayName="Codex CLI" />);
        expect(screen.getByText("v0.63.0")).toBeInTheDocument();
        expect(screen.getByRole("button", { name: "重新检测" })).toBeDisabled();
    });

    it("should offer one-click install when the CLI is missing and Bun is available", () => {
        const installAcpLibrary = vi.fn();
        render(
            <CliEnvironmentStatus
                env={env({ status: "not-installed", libraryInfo: library({ installed: false, version: null, installed_path: null, install_hint: "可点击下方“一键安装”" }), installAcpLibrary })}
                cliCommand="codex"
                displayName="Codex CLI"
            />,
        );
        expect(screen.getByText("未找到 Codex CLI")).toBeInTheDocument();
        fireEvent.click(screen.getByRole("button", { name: "一键安装" }));
        expect(installAcpLibrary).toHaveBeenCalled();
    });

    it("should show the install hint when the CLI must be installed manually", () => {
        render(
            <CliEnvironmentStatus
                env={env({ status: "external-required", libraryInfo: library({ cli_command: "kimi", package_name: "kimi", installed: false, version: null, installed_path: null, requires_external_install: true, install_hint: "请参考 Kimi Code CLI 官方文档安装" }) })}
                cliCommand="kimi"
                displayName="Kimi Code CLI"
            />,
        );
        expect(screen.getByText("未找到 Kimi Code CLI")).toBeInTheDocument();
        expect(screen.getByText(/请参考 Kimi Code CLI 官方文档安装/)).toBeInTheDocument();
        expect(screen.queryByRole("button", { name: "一键安装" })).not.toBeInTheDocument();
    });

    it("should show the failed executable and underlying reason when its version probe fails", () => {
        const reason = "C:/tools/claude.cmd --version 失败（退出码 1）：node.exe 找不到";
        render(<CliEnvironmentStatus env={env({ status: "probe-failed", libraryInfo: null, probeError: reason })}
            cliCommand="claude" displayName="Claude Code" />);
        expect(screen.getByText(reason)).toBeInTheDocument();
        expect(screen.queryByText(/手动安装|安装 Bun/)).not.toBeInTheDocument();
        expect(screen.getByRole("button", { name: "重新检测" })).toBeInTheDocument();
    });

    it("should not show another CLI's result while the newly selected CLI is being detected", () => {
        render(<CliEnvironmentStatus env={env({ status: "checking" })} cliCommand="kimi" displayName="Kimi Code CLI" />);
        expect(screen.queryByText("v0.63.0")).not.toBeInTheDocument();
        expect(screen.getByText(/正在检测/)).toBeInTheDocument();
    });
});

describe("versionNumber", () => {
    it("should extract the version when the output has a prefix or suffix", () => {
        expect(versionNumber("kimi, version 0.58.0")).toBe("0.58.0");
        expect(versionNumber("1.0.0-beta.2")).toBe("1.0.0-beta.2");
        expect(versionNumber("dev-build")).toBe("dev-build");
        expect(versionNumber("  ")).toBeNull();
    });
});
