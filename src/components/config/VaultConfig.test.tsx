import { describe, it, expect, vi, beforeEach } from "vitest";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import VaultConfig from "./VaultConfig";
import { toast } from "sonner";
import type { VaultCapability, VaultItem, VaultUsageEntry } from "@/data/Vault";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}) }));
vi.mock("sonner", () => ({ toast: { success: vi.fn(), error: vi.fn() } }));
vi.mock("@/hooks/use-platform", () => ({ useIsMobilePlatform: () => false }));

const capability: VaultCapability = {
    available: true, backend: "Windows 凭据管理器", reason: null,
    syncState: "not_configured", syncMessage: "未设置同步口令，只保存在本机", pendingFromOtherDevices: 0,
};

const item: VaultItem = {
    id: "a1", key: "github_token", name: "GitHub 令牌", description: "读取仓库",
    allowedSites: ["https://api.github.com"], syncEnabled: true,
    status: "ready", createdAt: 1, updatedAt: 1, lastUsedAt: null,
};

const usage: VaultUsageEntry[] = [{
    id: 1, itemId: "a1", conversationId: 3, assistantId: 2, tool: "操作 · execute_bash", destination: "本机命令",
    outcome: "denied", reason: "用户拒绝", createdAt: Date.now(),
}];

const simpleValueWarning = "凭据过于简单可能导致普通内容被误遮盖，建议使用较长且独特的值。";

function mockBackend(overrides: Record<string, unknown> = {}) {
    vi.mocked(invoke).mockImplementation(async (command: string) => {
        if (command in overrides) return overrides[command];
        if (command === "vault_capability") return capability;
        if (command === "vault_list") return [item];
        if (command === "vault_usage") return usage;
        if (command === "vault_references") return [];
        if (command === "vault_create") return { ...item, id: "b2", key: "my_token", name: "My Token" };
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

    it("should show entries as masked metadata with their placeholder", async () => {
        render(<VaultConfig />);
        expect(await screen.findByRole("heading", { name: "GitHub 令牌" })).toBeInTheDocument();
        expect(screen.getByRole("button", { name: "复制占位符 <vault:github_token>" })).toBeInTheDocument();
        expect(screen.getByText("https://api.github.com")).toBeInTheDocument();
        // 常态信息（加密后端、未设置同步口令）不占位置。
        expect(screen.queryByText(/Windows 凭据管理器/)).not.toBeInTheDocument();
        expect(screen.queryByText(/输入同步口令/)).not.toBeInTheDocument();
        expect(screen.queryByText(/发送时注入为/)).not.toBeInTheDocument();
    });

    it("should list where each use went and through which tool", async () => {
        render(<VaultConfig />);
        expect(await screen.findByText("本机命令")).toBeInTheDocument();
        expect(screen.getByText("操作 · execute_bash")).toBeInTheDocument();
        expect(screen.getByText("已拒绝")).toBeInTheDocument();
    });

    it("should create an entry with a suggested key, then forget the value", async () => {
        const user = userEvent.setup();
        mockBackend({ vault_list: [] });
        render(<VaultConfig />);
        await user.click(await screen.findByRole("button", { name: /新建条目/ }));
        await user.type(screen.getByLabelText("名称"), "My Token");
        expect(screen.getByLabelText("键")).toHaveValue("my_token");
        expect(screen.queryByText(/建议改成有意义的英文/)).not.toBeInTheDocument();
        const value = screen.getByLabelText("值");
        expect(value).toHaveAttribute("type", "password");
        await user.type(value, " ghp_example123 ");
        await user.click(screen.getByRole("button", { name: "确认" }));
        await waitFor(() => expect(invoke).toHaveBeenCalledWith("vault_create", {
            input: { name: "My Token", key: "my_token", description: "", value: " ghp_example123 ", allowedSites: [], syncEnabled: true },
        }));
        await user.click(await screen.findByRole("button", { name: /新建条目/ }));
        expect(screen.getByLabelText("值")).toHaveValue("");
    });

    it("should suggest a neutral key for names without latin letters", async () => {
        const user = userEvent.setup();
        mockBackend({ vault_list: [] });
        render(<VaultConfig />);
        await user.click(await screen.findByRole("button", { name: /新建条目/ }));
        await user.type(screen.getByLabelText("名称"), "身份证号");
        expect(screen.getByLabelText("键")).toHaveValue("entry");
        expect(screen.getByText(/建议改成有意义的英文/)).toBeInTheDocument();
        await user.clear(screen.getByLabelText("键"));
        await user.type(screen.getByLabelText("键"), "ID");
        await user.type(screen.getByLabelText("值"), "x");
        await user.click(screen.getByRole("button", { name: "确认" }));
        expect(await screen.findByRole("alert")).toHaveTextContent(/小写字母/);
        expect(invoke).not.toHaveBeenCalledWith("vault_create", expect.anything());
    });

    it("should show a nonblocking hint below a simple new value and clear it after cancel or save", async () => {
        const user = userEvent.setup();
        render(<VaultConfig />);
        await user.click(await screen.findByRole("button", { name: /新建条目/ }));
        expect(screen.queryByText(simpleValueWarning)).not.toBeInTheDocument();
        const value = screen.getByLabelText("值");
        fireEvent.change(value, { target: { value: "text" } });
        const warning = screen.getByText(simpleValueWarning);
        expect(value.parentElement?.nextElementSibling).toBe(warning);
        expect(value).toHaveAccessibleDescription(simpleValueWarning);
        expect(value).toHaveAttribute("type", "password");
        expect(screen.queryByText("text", { exact: true })).not.toBeInTheDocument();
        expect(invoke).not.toHaveBeenCalledWith("vault_create", expect.anything());

        fireEvent.change(value, { target: { value: "vault-token_d7Q9z!K2" } });
        expect(screen.queryByText(simpleValueWarning)).not.toBeInTheDocument();
        expect(value).not.toHaveAttribute("aria-describedby");
        fireEvent.change(value, { target: { value: "   " } });
        expect(screen.queryByText(simpleValueWarning)).not.toBeInTheDocument();
        fireEvent.change(value, { target: { value: "text" } });
        await user.click(screen.getByRole("button", { name: "取消" }));
        await user.click(screen.getByRole("button", { name: /新建条目/ }));
        expect(screen.getByLabelText("值")).toHaveValue("");
        expect(screen.queryByText(simpleValueWarning)).not.toBeInTheDocument();

        await user.type(within(screen.getByRole("dialog")).getByLabelText("名称"), "My Token");
        fireEvent.change(screen.getByLabelText("值"), { target: { value: "  text  " } });
        expect(screen.getByText(simpleValueWarning)).toBeInTheDocument();
        await user.click(screen.getByRole("button", { name: "确认" }));
        await waitFor(() => expect(invoke).toHaveBeenCalledWith("vault_create", {
            input: { name: "My Token", key: "my_token", description: "", value: "  text  ", allowedSites: [], syncEnabled: true },
        }));
        await user.click(await screen.findByRole("button", { name: /新建条目/ }));
        expect(screen.getByLabelText("值")).toHaveValue("");
        expect(screen.queryByText(simpleValueWarning)).not.toBeInTheDocument();
    });

    it("should warn for simple replacement values without exposing them or blocking replacement", async () => {
        const user = userEvent.setup();
        render(<VaultConfig />);
        await user.click(await screen.findByRole("button", { name: "替换值" }));
        expect(screen.queryByText(simpleValueWarning)).not.toBeInTheDocument();
        const value = screen.getByLabelText("新的值");
        const repetitive = "abcd".repeat(8);
        fireEvent.change(value, { target: { value: repetitive } });
        expect(value.parentElement?.nextElementSibling).toBe(screen.getByText(simpleValueWarning));
        expect(value).toHaveAccessibleDescription(simpleValueWarning);
        expect(value).toHaveAttribute("type", "password");
        expect(screen.queryByText(repetitive)).not.toBeInTheDocument();
        await user.keyboard("{Escape}");
        expect(invoke).not.toHaveBeenCalledWith("vault_replace_secret", expect.anything());
        await user.click(screen.getByRole("button", { name: "替换值" }));
        expect(screen.getByLabelText("新的值")).toHaveValue("");
        expect(screen.queryByText(simpleValueWarning)).not.toBeInTheDocument();

        fireEvent.change(screen.getByLabelText("新的值"), { target: { value: "text" } });
        expect(screen.getByText(simpleValueWarning)).toBeInTheDocument();
        fireEvent.change(screen.getByLabelText("新的值"), { target: { value: "vault-token_d7Q9z!K2" } });
        expect(screen.queryByText(simpleValueWarning)).not.toBeInTheDocument();
        fireEvent.change(screen.getByLabelText("新的值"), { target: { value: "" } });
        expect(screen.queryByText(simpleValueWarning)).not.toBeInTheDocument();
        fireEvent.change(screen.getByLabelText("新的值"), { target: { value: "  body  " } });
        expect(screen.getByText(simpleValueWarning)).toBeInTheDocument();
        await user.click(screen.getByRole("button", { name: "确认" }));
        await waitFor(() => expect(invoke).toHaveBeenCalledWith("vault_replace_secret", { id: "a1", value: "  body  " }));
        await user.click(screen.getByRole("button", { name: "替换值" }));
        expect(screen.getByLabelText("新的值")).toHaveValue("");
        expect(screen.queryByText(simpleValueWarning)).not.toBeInTheDocument();
    });

    it("should not prefill the old value when replacing", async () => {
        const user = userEvent.setup();
        render(<VaultConfig />);
        await user.click(await screen.findByRole("button", { name: "替换值" }));
        expect(screen.getByLabelText("新的值")).toHaveValue("");
        await user.type(screen.getByLabelText("新的值"), " new-value ");
        await user.click(screen.getByRole("button", { name: "确认" }));
        await waitFor(() => expect(invoke).toHaveBeenCalledWith("vault_replace_secret", { id: "a1", value: " new-value " }));
    });

    it("should list configs that use the entry and confirm a changed target", async () => {
        const user = userEvent.setup();
        mockBackend({
            vault_references: [
                { kind: "provider", configId: 3, name: "Provider「OpenAI」", target: "https://api.openai.com", approved: true, problem: null, snapshot: "provider-snapshot" },
                { kind: "mcp", configId: 7, name: "MCP「GitHub」", target: "https://other.example", approved: false, problem: null, snapshot: "mcp-snapshot" },
            ],
            vault_confirm_reference: null,
        });
        render(<VaultConfig />);
        expect(await screen.findByText("Provider「OpenAI」")).toBeInTheDocument();
        expect(screen.getByText("已确认")).toBeInTheDocument();
        expect(screen.getAllByText("配置已变化")).toHaveLength(1);
        await user.click(screen.getByRole("button", { name: "确认 MCP「GitHub」 使用此条目访问 https://other.example" }));
        await waitFor(() => expect(invoke).toHaveBeenCalledWith("vault_confirm_reference", { itemId: "a1", kind: "mcp", configId: 7, expectedSnapshot: "mcp-snapshot" }));
    });

    it("should refresh a rejected stale snapshot without approving the new configuration", async () => {
        const user = userEvent.setup();
        const oldReference = { kind: "mcp", configId: 7, name: "MCP「GitHub」", target: "https://old.example", approved: false, problem: null, snapshot: "old-snapshot" };
        const newReference = { ...oldReference, target: "https://new.example", snapshot: "new-snapshot" };
        let changed = false;
        vi.mocked(invoke).mockImplementation(async (command: string) => {
            if (command === "vault_capability") return capability;
            if (command === "vault_list") return [item];
            if (command === "vault_usage") return [];
            if (command === "vault_references") return [changed ? newReference : oldReference];
            if (command === "vault_confirm_reference") {
                changed = true;
                throw new Error("配置在展示后已变化，本次未授权。请检查最新配置后重新确认");
            }
            throw new Error(`Unexpected command: ${command}`);
        });
        render(<VaultConfig />);
        await user.click(await screen.findByRole("button", { name: "确认 MCP「GitHub」 使用此条目访问 https://old.example" }));
        await waitFor(() => expect(toast.error).toHaveBeenCalledWith(expect.stringContaining("重新确认")));
        expect(toast.success).not.toHaveBeenCalled();
        expect(await screen.findByRole("button", { name: "确认 MCP「GitHub」 使用此条目访问 https://new.example" })).toBeInTheDocument();
        const confirmationCalls = vi.mocked(invoke).mock.calls.filter(([command]) => command === "vault_confirm_reference");
        expect(confirmationCalls).toEqual([["vault_confirm_reference", { itemId: "a1", kind: "mcp", configId: 7, expectedSnapshot: "old-snapshot" }]]);
    });

    it("should not offer the previous entry's confirmation while the newly selected entry loads", async () => {
        const user = userEvent.setup();
        const second = { ...item, id: "b2", key: "second_token", name: "Second token" };
        const oldReference = { kind: "mcp", configId: 7, name: "MCP「GitHub」", target: "https://old.example", approved: false, problem: null, snapshot: "entry-a-snapshot" };
        let finishSecond!: (value: unknown) => void;
        const secondReferences = new Promise((resolve) => { finishSecond = resolve; });
        vi.mocked(invoke).mockImplementation(async (command: string, args) => {
            if (command === "vault_capability") return capability;
            if (command === "vault_list") return [item, second];
            if (command === "vault_usage") return [];
            if (command === "vault_references") return (args as { id: string }).id === "a1" ? [oldReference] : secondReferences;
            if (command === "vault_confirm_reference") return null;
            throw new Error(`Unexpected command: ${command}`);
        });
        render(<VaultConfig />);
        await screen.findByRole("button", { name: "确认 MCP「GitHub」 使用此条目访问 https://old.example" });
        await user.click(screen.getByRole("button", { name: /Second token/ }));
        expect(await screen.findByRole("heading", { name: "Second token" })).toBeInTheDocument();
        expect(screen.queryByRole("button", { name: "确认 MCP「GitHub」 使用此条目访问 https://old.example" })).not.toBeInTheDocument();
        expect(invoke).not.toHaveBeenCalledWith("vault_confirm_reference", expect.anything());
        await act(async () => { finishSecond([{ ...oldReference, snapshot: "entry-b-snapshot" }]); });
        await user.click(await screen.findByRole("button", { name: "确认 MCP「GitHub」 使用此条目访问 https://old.example" }));
        await waitFor(() => expect(invoke).toHaveBeenCalledWith("vault_confirm_reference", {
            itemId: "b2", kind: "mcp", configId: 7, expectedSnapshot: "entry-b-snapshot",
        }));
    });

    it("should name the configs that stop working before deleting", async () => {
        const user = userEvent.setup();
        mockBackend({
            vault_references: [{ kind: "provider", configId: 3, name: "Provider「OpenAI」", target: "https://api.openai.com", approved: true, problem: null, snapshot: "provider-snapshot" }],
        });
        render(<VaultConfig />);
        await screen.findByText("Provider「OpenAI」");
        await user.click(screen.getByRole("button", { name: "删除" }));
        expect(await screen.findByText(/以下配置正在使用它，删除后将无法工作：Provider「OpenAI」/)).toBeInTheDocument();
        expect(invoke).not.toHaveBeenCalledWith("vault_delete", expect.anything());
    });

    it("should explain the three steps when the vault is empty", async () => {
        mockBackend({ vault_list: [] });
        render(<VaultConfig />);
        expect(await screen.findByText("存进来")).toBeInTheDocument();
        expect(screen.getByText("让助手用")).toBeInTheDocument();
        expect(screen.getByText("由你批准")).toBeInTheDocument();
        expect(screen.getByRole("button", { name: /新建条目/ })).toBeInTheDocument();
    });

    it("should edit name, key, description and sync in one dialog and send only the changes", async () => {
        const user = userEvent.setup();
        mockBackend({ vault_update: item });
        render(<VaultConfig />);
        await user.click(await screen.findByRole("button", { name: "编辑" }));
        const dialog = screen.getByRole("dialog");
        expect(within(dialog).getByLabelText("键")).toHaveValue("github_token");
        await user.clear(within(dialog).getByLabelText("名称"));
        await user.type(within(dialog).getByLabelText("名称"), "GitHub PAT");
        await user.click(within(dialog).getByRole("switch", { name: "同步到我的其他设备" }));
        await user.click(within(dialog).getByRole("button", { name: "确认" }));
        await waitFor(() => expect(invoke).toHaveBeenCalledWith("vault_update", { id: "a1", input: { name: "GitHub PAT", syncEnabled: false } }));
        await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    });

    it("should block an invalid key in the edit dialog", async () => {
        const user = userEvent.setup();
        render(<VaultConfig />);
        await user.click(await screen.findByRole("button", { name: "编辑" }));
        const key = within(screen.getByRole("dialog")).getByLabelText("键");
        await user.clear(key);
        await user.type(key, "Bad");
        expect(screen.getAllByText(/小写字母/).length).toBeGreaterThan(0);
        await user.click(screen.getByRole("button", { name: "确认" }));
        expect(invoke).not.toHaveBeenCalledWith("vault_update", expect.anything());
    });

    it("should show the latest uses first and reveal the rest on demand", async () => {
        const user = userEvent.setup();
        const many = Array.from({ length: 7 }, (_, index) => ({ ...usage[0], id: index + 1, destination: `https://site${index + 1}.example` }));
        mockBackend({ vault_usage: many });
        render(<VaultConfig />);
        expect(await screen.findByText("https://site5.example")).toBeInTheDocument();
        expect(screen.queryByText("https://site6.example")).not.toBeInTheDocument();
        await user.click(screen.getByRole("button", { name: "显示全部 7 条" }));
        expect(screen.getByText("https://site7.example")).toBeInTheDocument();
    });

    it("should show the result of every use", async () => {
        mockBackend({ vault_usage: [{ ...usage[0], id: 2, outcome: "ok", destination: "https://ok.example" }, usage[0]] });
        render(<VaultConfig />);
        expect(await screen.findByText("https://ok.example")).toBeInTheDocument();
        expect(screen.getByText("完成")).toBeInTheDocument();
        expect(screen.getByText("已拒绝")).toBeInTheDocument();
    });

    it("should show the facts of an entry that has never been used", async () => {
        mockBackend({ vault_usage: [], vault_list: [{ ...item, description: "" }] });
        render(<VaultConfig />);
        await screen.findByRole("heading", { name: "GitHub 令牌" });
        expect(screen.getByText("从未使用")).toBeInTheDocument();
        expect(screen.getByText("还没有使用记录。")).toBeInTheDocument();
        expect(screen.getByText("还没有配置引用它。")).toBeInTheDocument();
        expect(screen.getByText(/未填写。助手靠名称、键和说明判断该用哪个条目/)).toBeInTheDocument();
        expect(screen.getByText("已开启，设置同步口令后开始同步")).toBeInTheDocument();
    });

    it("should ask for the sync passphrase only when something waits for it", async () => {
        mockBackend({ vault_capability: { ...capability, syncState: "needs_passphrase", syncMessage: "其他设备已开启加密同步，输入同步口令后才能同步条目" } });
        render(<VaultConfig />);
        expect(await screen.findByText("其他设备已开启同步，输入同步口令后才能同步。")).toBeInTheDocument();
        expect(screen.getByRole("button", { name: /前往数据与同步/ })).toBeInTheDocument();
    });

    it("should say where each entry is kept", async () => {
        mockBackend({
            vault_capability: { ...capability, syncState: "ready", syncMessage: "端到端加密同步已开启" },
            vault_list: [item, { ...item, id: "b2", key: "local_only", name: "Local only", syncEnabled: false }],
        });
        const user = userEvent.setup();
        render(<VaultConfig />);
        expect(await screen.findByText("同步到我的其他设备（端到端加密）")).toBeInTheDocument();
        await user.click(screen.getByRole("button", { name: /Local only/ }));
        expect(await screen.findByText("只保存在本机")).toBeInTheDocument();
    });

    it("should filter the list by name, key or description", async () => {
        const user = userEvent.setup();
        mockBackend({ vault_list: [item, { ...item, id: "b2", key: "id_card", name: "身份证号", description: "" }] });
        render(<VaultConfig />);
        await user.type(await screen.findByPlaceholderText("搜索条目..."), "id_card");
        const list = screen.getByRole("navigation", { name: "凭据保险箱" });
        expect(within(list).getByRole("button", { name: "身份证号" })).toBeInTheDocument();
        expect(within(list).queryByRole("button", { name: "GitHub 令牌" })).not.toBeInTheDocument();
    });

    it("should explain a key change only after the key is edited", async () => {
        const user = userEvent.setup();
        render(<VaultConfig />);
        await user.click(await screen.findByRole("button", { name: "编辑" }));
        expect(screen.queryByText(/助手要改用新的占位符/)).not.toBeInTheDocument();
        await user.type(within(screen.getByRole("dialog")).getByLabelText("键"), "_v2");
        expect(screen.getByText(/助手要改用新的占位符/)).toBeInTheDocument();
    });

    it("should list entries one line each in the shared themeable list", async () => {
        const locked = { ...item, id: "b2", key: "nas_password", name: "NAS 密码", status: "locked" as const };
        mockBackend({ vault_list: [item, locked] });
        const { container } = render(<VaultConfig />);
        await screen.findByRole("heading", { name: "GitHub 令牌" });
        const list = screen.getByRole("navigation", { name: "凭据保险箱" });
        const rows = Array.from(list.querySelectorAll('[data-aipp-slot="config-list-item"]'));
        expect(rows.map((row) => row.textContent)).toEqual(["GitHub 令牌", "NAS 密码待解锁"]);
        expect(rows[0]).toHaveAttribute("aria-current", "true");
        expect(within(list).queryByText("github_token")).not.toBeInTheDocument();
        expect(within(list).getByRole("button", { name: "新建条目" })).toBeInTheDocument();
        // 主题通过这些 slot 定制外观，名称是对外约定。
        for (const slot of ["config-list", "config-list-toolbar", "config-list-search", "config-list-actions", "config-list-items", "vault-detail", "vault-detail-header", "vault-section-basic", "vault-section-sites", "vault-section-references", "vault-section-usage"]) {
            expect(container.querySelector(`[data-aipp-slot="${slot}"]`), slot).not.toBeNull();
        }
    });

    it("should switch to list then detail in a narrow window", async () => {
        const user = userEvent.setup();
        Object.defineProperty(window, "innerWidth", { value: 600, configurable: true, writable: true });
        render(<VaultConfig />);
        await user.click(await screen.findByRole("button", { name: "GitHub 令牌" }));
        expect(await screen.findByRole("heading", { name: "GitHub 令牌" })).toBeInTheDocument();
        expect(screen.queryByRole("navigation", { name: "凭据保险箱" })).not.toBeInTheDocument();
    });

    it("should explain when the device has no secure storage", async () => {
        mockBackend({ vault_capability: { ...capability, available: false, backend: null, reason: "此设备没有可用的系统安全存储" } });
        render(<VaultConfig />);
        expect(await screen.findByText("凭据保险箱不可用")).toBeInTheDocument();
        expect(screen.getByText("此设备没有可用的系统安全存储")).toBeInTheDocument();
        expect(invoke).not.toHaveBeenCalledWith("vault_list");
    });
});
