import { describe, it, expect, vi, beforeEach } from "vitest";
import { act, render, renderHook, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { VaultUseCard } from "./VaultUseCard";
import { useVaultUseRequests } from "@/hooks/useVaultUseRequests";
import type { VaultUseRequest } from "@/data/Vault";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

const request: VaultUseRequest = {
    requestId: "r1", conversationId: 5, assistantName: "编程助手",
    credentialId: "a1", credentialName: "GitHub 个人令牌", credentialHandle: "github_pat",
    method: "GET", url: "https://api.github.com/user/repos?per_page=100", origin: "https://api.github.com",
    insecure: false, headerNames: ["Accept"], hasBody: false, createdAt: 1,
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

    it("should show who wants which credential for which site and send the decision", async () => {
        const user = userEvent.setup();
        const onRespond = vi.fn(async () => {});
        render(<VaultUseCard request={request} onRespond={onRespond} />);
        expect(screen.getByText("编程助手")).toBeInTheDocument();
        expect(screen.getByText("GitHub 个人令牌")).toBeInTheDocument();
        expect(screen.getByText(/api\.github\.com\/user\/repos/)).toBeInTheDocument();
        await user.click(screen.getByRole("button", { name: "总是允许 api.github.com" }));
        expect(onRespond).toHaveBeenCalledWith("r1", "always");
    });

    it("should warn about plain http", () => {
        render(<VaultUseCard request={{ ...request, insecure: true, origin: "http://192.168.1.10:8123" }} onRespond={vi.fn(async () => {})} />);
        expect(screen.getByText(/未加密的 http/)).toBeInTheDocument();
        expect(screen.getByRole("button", { name: "总是允许 192.168.1.10:8123" })).toBeInTheDocument();
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
