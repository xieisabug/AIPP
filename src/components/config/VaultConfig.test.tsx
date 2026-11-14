import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import VaultConfig from "./VaultConfig";
import type { VaultCapability, VaultItem } from "@/data/Vault";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));
vi.mock("@/hooks/use-platform", () => ({ useIsMobilePlatform: () => false }));

const capability: VaultCapability = {
    available: true, backend: "Windows 凭据管理器", reason: null,
    syncState: "not_configured", syncMessage: "未设置同步口令，凭据只保存在本机", pendingFromOtherDevices: 0,
};

const item: VaultItem = {
    id: "a1", handle: "github_pat", name: "GitHub 个人令牌", description: "读取仓库", authKind: "bearer",
    injection: ["Authorization: Bearer <值>"], fields: [{ key: "token", label: "令牌" }], allowedSites: ["https://api.github.com"], syncEnabled: true,
    status: "ready", createdAt: 1, updatedAt: 1, lastUsedAt: null,
};

function mockBackend(overrides: Record<string, unknown> = {}) {
    vi.mocked(invoke).mockImplementation(async (command: string) => {
        if (command in overrides) return overrides[command];
        if (command === "vault_capability") return capability;
        if (command === "vault_list") return [item];
        if (command === "vault_usage") return [];
        if (command === "vault_references") return [];
        if (command === "vault_create") return { ...item, id: "b2", handle: "new_token", name: "New Token" };
        if (command === "vault_replace_secret") return item;
        throw new Error(`Unexpected command: ${command}`);
    });
}

describe("VaultConfig", () => {
    beforeEach(() => {
        vi.clearAllMocks();
        Object.defineProperty(window, "innerWidth", { value: 1400, configurable: true, writable: true });
        mockBackend();
    });

    it("should show stored credentials as masked metadata only", async () => {
        render(<VaultConfig />);
        expect(await screen.findByRole("heading", { name: "GitHub 个人令牌" })).toBeInTheDocument();
        expect(screen.getByText("Authorization: Bearer <值>")).toBeInTheDocument();
        expect(screen.getByText("https://api.github.com")).toBeInTheDocument();
        expect(screen.getByText(/本机加密 · Windows 凭据管理器/)).toBeInTheDocument();
        expect(screen.getByText(/不会再显示/)).toBeInTheDocument();
    });

    it("should create a credential with a suggested handle and site, then forget the value", async () => {
        const user = userEvent.setup();
        mockBackend({ vault_list: [] });
        render(<VaultConfig />);
        await user.click(await screen.findByRole("button", { name: /新建凭据/ }));
        await user.type(screen.getByLabelText("名称"), "My Token");
        expect(screen.getByLabelText("引用名")).toHaveValue("my_token");
        await user.type(screen.getByLabelText("令牌"), "ghp_example123");
        await user.click(screen.getByRole("button", { name: "建议：https://api.github.com" }));
        await user.click(screen.getByRole("button", { name: "确认" }));
        await waitFor(() => expect(invoke).toHaveBeenCalledWith("vault_create", {
            input: expect.objectContaining({
                name: "My Token",
                handle: "my_token",
                auth: { kind: "bearer", token: "ghp_example123" },
                allowedSites: ["https://api.github.com"],
            }),
        }));
        await user.click(await screen.findByRole("button", { name: /新建凭据/ }));
        expect(screen.getByLabelText("令牌")).toHaveValue("");
    });

    it("should not prefill the old value when replacing", async () => {
        const user = userEvent.setup();
        render(<VaultConfig />);
        await user.click(await screen.findByRole("button", { name: "替换" }));
        expect(screen.getByLabelText("令牌")).toHaveValue("");
        await user.type(screen.getByLabelText("令牌"), "new-value");
        await user.click(screen.getByRole("button", { name: "确认" }));
        await waitFor(() => expect(invoke).toHaveBeenCalledWith("vault_replace_secret", { id: "a1", auth: { kind: "bearer", token: "new-value" } }));
    });

    it("should list configs that use the credential and confirm a changed target", async () => {
        const user = userEvent.setup();
        mockBackend({
            vault_references: [
                { kind: "provider", configId: 3, name: "Provider「OpenAI」", target: "https://api.openai.com", approved: true, problem: null },
                { kind: "mcp", configId: 7, name: "MCP「GitHub」", target: "https://other.example", approved: false, problem: null },
            ],
            vault_confirm_reference: null,
        });
        render(<VaultConfig />);
        expect(await screen.findByText("Provider「OpenAI」")).toBeInTheDocument();
        expect(screen.getByText("已确认")).toBeInTheDocument();
        expect(screen.getByText("配置已变化")).toBeInTheDocument();
        await user.click(screen.getByRole("button", { name: "确认 MCP「GitHub」 使用此凭据访问 https://other.example" }));
        await waitFor(() => expect(invoke).toHaveBeenCalledWith("vault_confirm_reference", { kind: "mcp", configId: 7 }));
    });

    it("should name the configs that stop working before deleting", async () => {
        const user = userEvent.setup();
        mockBackend({
            vault_references: [{ kind: "provider", configId: 3, name: "Provider「OpenAI」", target: "https://api.openai.com", approved: true, problem: null }],
        });
        render(<VaultConfig />);
        await screen.findByText("Provider「OpenAI」");
        await user.click(screen.getByRole("button", { name: "删除" }));
        expect(await screen.findByText(/以下配置正在使用它，删除后将无法工作：Provider「OpenAI」/)).toBeInTheDocument();
        expect(invoke).not.toHaveBeenCalledWith("vault_delete", expect.anything());
    });

    it("should explain when the device has no secure storage", async () => {
        mockBackend({ vault_capability: { ...capability, available: false, backend: null, reason: "此设备没有可用的系统安全存储" } });
        render(<VaultConfig />);
        expect(await screen.findByText("凭据保险箱不可用")).toBeInTheDocument();
        expect(screen.getByText("此设备没有可用的系统安全存储")).toBeInTheDocument();
        expect(invoke).not.toHaveBeenCalledWith("vault_list");
    });
});
