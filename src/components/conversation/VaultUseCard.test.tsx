import { describe, it, expect, vi, beforeEach } from "vitest";
import { act, render, renderHook, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { VaultUseCard, vaultInlineItems } from "./VaultUseCard";
import { useVaultUseRequests } from "@/hooks/useVaultUseRequests";
import type { VaultUseRequest } from "@/data/Vault";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

const request: VaultUseRequest = {
    requestId: "r1", conversationId: 5, messageId: 11, assistantName: "编程助手",
    entries: [{ id: "a1", key: "github_token", name: "GitHub 令牌" }],
    tool: "保险箱 · vault_http_request",
    destination: { kind: "site", label: "网页请求 GET https://api.github.com/user/repos", origin: "https://api.github.com", insecure: false },
    preview: JSON.stringify({ url: "https://api.github.com/user/repos", headers: { Authorization: "Bearer <vault:github_token>" } }, null, 2),
    previewKind: "json", canAlways: true, createdAt: 1,
};

const shellRequest: VaultUseRequest = {
    ...request,
    requestId: "r-shell",
    tool: "操作 · execute_bash",
    destination: { kind: "local", label: "本机命令", origin: null, insecure: false },
    preview: 'curl -H "Authorization: Bearer <vault:github_token>" https://api.github.com/user',
    previewKind: "command",
    canAlways: false,
};

type Handler = (event: { payload: unknown }) => void;
const handlers = new Map<string, Handler>();

describe("VaultUseCard", () => {
    beforeEach(() => {
        vi.clearAllMocks();
        handlers.clear();
        vi.mocked(listen).mockImplementation(async (event: string, handler: unknown) => {
            handlers.set(event, handler as Handler);
            return () => handlers.delete(event);
        });
    });

    it("should show who uses which entry where and send the decision", async () => {
        const user = userEvent.setup();
        const onRespond = vi.fn(async () => {});
        render(<VaultUseCard request={request} onRespond={onRespond} />);
        expect(screen.getByText("助手「编程助手」要使用保险箱")).toBeInTheDocument();
        expect(screen.getByText("GitHub 令牌")).toBeInTheDocument();
        expect(screen.getByText("网页请求 GET https://api.github.com/user/repos")).toBeInTheDocument();
        // Network destinations start collapsed.
        expect(screen.getByRole("button", { name: "调用详情" })).toHaveAttribute("aria-expanded", "false");
        await user.click(screen.getByRole("button", { name: "总是允许用于 api.github.com" }));
        expect(onRespond).toHaveBeenCalledWith("r1", "always");
    });

    it("should offer only a one-time approval for local commands and show the command with chips", async () => {
        const user = userEvent.setup();
        const onRespond = vi.fn(async () => {});
        render(<VaultUseCard request={shellRequest} onRespond={onRespond} />);
        expect(screen.getByText("本机命令")).toBeInTheDocument();
        expect(screen.getByText(/本机程序能读到这个值/)).toBeInTheDocument();
        expect(screen.queryByRole("button", { name: /总是允许/ })).not.toBeInTheDocument();
        // The command is visible right away, with the placeholder shown as the entry name.
        expect(screen.getByRole("button", { name: "调用详情" })).toHaveAttribute("aria-expanded", "true");
        expect(screen.getByText(/curl -H "Authorization: Bearer/)).toBeInTheDocument();
        expect(screen.getAllByText("GitHub 令牌")).toHaveLength(2);
        expect(screen.queryByText(/<vault:/)).not.toBeInTheDocument();
        await user.click(screen.getByRole("button", { name: "允许这一次" }));
        expect(onRespond).toHaveBeenCalledWith("r-shell", "once");
    });

    it("should warn about plain http", () => {
        const insecure = { ...request, destination: { ...request.destination, origin: "http://192.168.1.10:8123", insecure: true } };
        render(<VaultUseCard request={insecure} onRespond={vi.fn(async () => {})} />);
        expect(screen.getByText(/未加密的 http/)).toBeInTheDocument();
        expect(screen.getByRole("button", { name: "总是允许用于 192.168.1.10:8123" })).toBeInTheDocument();
    });

    it("should attach the card to the message that made the call", () => {
        const [item] = vaultInlineItems([request], vi.fn(async () => {}));
        expect(item.messageId).toBe(11);
        expect(item.key).toBe("vault-use-r1");
    });

    it("should track requests for the shown conversation and drop them once resolved", async () => {
        vi.mocked(invoke).mockImplementation(async (command: string) => {
            if (command === "vault_list_use_requests") return [request, { ...request, requestId: "other", conversationId: 9 }];
            return undefined;
        });
        const { result } = renderHook(() => useVaultUseRequests(5));
        await waitFor(() => expect(result.current.requests.map((r) => r.requestId)).toEqual(["r1"]));
        await waitFor(() => expect(handlers.has("vault-use-request")).toBe(true));
        act(() => handlers.get("vault-use-request")?.({ payload: { ...request, requestId: "r2" } }));
        act(() => handlers.get("vault-use-request")?.({ payload: { ...request, requestId: "r3", conversationId: 9 } }));
        expect(result.current.requests.map((r) => r.requestId)).toEqual(["r1", "r2"]);
        act(() => handlers.get("vault-use-resolved")?.({ payload: { requestId: "r1", conversationId: 5 } }));
        expect(result.current.requests.map((r) => r.requestId)).toEqual(["r2"]);
        await act(() => result.current.respond("r2", "deny"));
        expect(invoke).toHaveBeenCalledWith("vault_respond_use_request", { requestId: "r2", decision: "deny" });
    });
});
