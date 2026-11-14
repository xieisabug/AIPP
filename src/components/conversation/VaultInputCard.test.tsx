import { describe, it, expect, vi, beforeEach } from "vitest";
import { act, render, renderHook, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { VaultInputCard, vaultInputInlineItems } from "./VaultInputCard";
import { useVaultInputRequests } from "@/hooks/useVaultInputRequests";
import type { VaultInputRequest } from "@/data/Vault";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));

const request: VaultInputRequest = {
    requestId: "in1", conversationId: 5, messageId: 12, assistantName: "研究员",
    purpose: "为你添加 12306 乘车人",
    fields: [
        { key: "id_card", name: "身份证号", description: "18 位" },
        { key: "phone", name: "手机号", description: "" },
    ],
    createdAt: 1,
};

type Handler = (event: { payload: unknown }) => void;
const handlers = new Map<string, Handler>();

describe("VaultInputCard", () => {
    beforeEach(() => {
        vi.clearAllMocks();
        handlers.clear();
        vi.mocked(listen).mockImplementation(async (event: string, handler: unknown) => {
            handlers.set(event, handler as Handler);
            return () => handlers.delete(event);
        });
    });

    it("should ask with masked fields and submit straight to the vault", async () => {
        const user = userEvent.setup();
        const onSubmit = vi.fn(async () => {});
        render(<VaultInputCard request={request} onSubmit={onSubmit} onDecline={vi.fn(async () => {})} />);
        expect(screen.getByText("助手「研究员」请你填写")).toBeInTheDocument();
        expect(screen.getByText("用途：为你添加 12306 乘车人")).toBeInTheDocument();
        const idCard = screen.getByLabelText("身份证号");
        expect(idCard).toHaveAttribute("type", "password");
        expect(screen.getByLabelText("手机号")).toHaveAttribute("type", "password");
        const save = screen.getByRole("button", { name: "保存并继续" });
        expect(save).toBeDisabled();
        await user.type(idCard, "110101199001011234");
        expect(save).toBeDisabled();
        await user.type(screen.getByLabelText("手机号"), "13800000000");
        await user.click(screen.getByRole("checkbox"));
        await user.click(save);
        expect(onSubmit).toHaveBeenCalledWith("in1", { id_card: "110101199001011234", phone: "13800000000" }, false);
        // Nothing typed stays in the form after a successful save.
        await waitFor(() => expect(screen.getByLabelText("身份证号")).toHaveValue(""));
    });

    it("should keep the form open with the error when saving fails", async () => {
        const user = userEvent.setup();
        const onSubmit = vi.fn(async () => { throw "键 phone 已被使用"; });
        render(<VaultInputCard request={{ ...request, fields: [request.fields[1]] }} onSubmit={onSubmit} onDecline={vi.fn(async () => {})} />);
        await user.type(screen.getByLabelText("手机号"), "13800000000");
        await user.click(screen.getByRole("button", { name: "保存并继续" }));
        expect(await screen.findByRole("alert")).toHaveTextContent("键 phone 已被使用");
        expect(screen.getByLabelText("手机号")).toHaveValue("13800000000");
    });

    it("should preserve surrounding whitespace when submitting a secret", async () => {
        const user = userEvent.setup();
        const onSubmit = vi.fn(async () => {});
        render(<VaultInputCard request={{ ...request, fields: [{ key: "password", name: "密码", description: "" }] }} onSubmit={onSubmit} onDecline={vi.fn(async () => {})} />);
        await user.type(screen.getByLabelText("密码"), "  password with spaces  ");
        await user.click(screen.getByRole("button", { name: "保存并继续" }));
        expect(onSubmit).toHaveBeenCalledWith("in1", { password: "  password with spaces  " }, true);
    });

    it("should let the user decline", async () => {
        const user = userEvent.setup();
        const onDecline = vi.fn(async () => {});
        render(<VaultInputCard request={request} onSubmit={vi.fn(async () => {})} onDecline={onDecline} />);
        await user.click(screen.getByRole("button", { name: "拒绝" }));
        expect(onDecline).toHaveBeenCalledWith("in1");
    });

    it("should attach the form to the message that asked", () => {
        const [item] = vaultInputInlineItems([request], vi.fn(async () => {}), vi.fn(async () => {}));
        expect(item.messageId).toBe(12);
    });

    it("should track input requests and send answers through the vault commands", async () => {
        vi.mocked(invoke).mockImplementation(async (command: string) => {
            if (command === "vault_list_input_requests") return [request, { ...request, requestId: "other", conversationId: 9 }];
            return undefined;
        });
        const { result } = renderHook(() => useVaultInputRequests(5));
        await waitFor(() => expect(result.current.requests.map((r) => r.requestId)).toEqual(["in1"]));
        await waitFor(() => expect(handlers.has("vault-input-resolved")).toBe(true));
        act(() => handlers.get("vault-input-resolved")?.({ payload: { requestId: "in1", conversationId: 5 } }));
        expect(result.current.requests).toEqual([]);
        await act(() => result.current.submit("in2", { phone: "1" }, true));
        expect(invoke).toHaveBeenCalledWith("vault_submit_input", { requestId: "in2", values: { phone: "1" }, sync: true });
        await act(() => result.current.decline("in3"));
        expect(invoke).toHaveBeenCalledWith("vault_decline_input", { requestId: "in3" });
    });
});
