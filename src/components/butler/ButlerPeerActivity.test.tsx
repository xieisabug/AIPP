import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { emit } from "@tauri-apps/api/event";
import { toast } from "sonner";
import { afterEach, describe, expect, it, vi } from "vitest";
import { clearAllMockHandlers, mockInvokeHandler } from "@/__tests__/mocks/tauri";
import ButlerPeerActivity from "./ButlerPeerActivity";

vi.mock("sonner", () => ({ toast: { error: vi.fn() } }));

afterEach(() => { clearAllMockHandlers(); vi.clearAllMocks(); });

const failedExchange = (receiptState: string | null = "failed") => ({
    id: 4, root: 10, owner: 1, target: 2, sender: "写作", recipient: "审稿",
    state: "failed", round_no: 2, max_rounds: 3, body: "核对第二稿",
    error: "执行结果不明", receipt_state: receiptState,
});

describe("Butler peer activity", () => {
    it("should show one status and keep the exchange text inside the detail dialog", async () => {
        mockInvokeHandler("list_butler_peer_exchanges", () => [{
            id: 4, root: 10, owner: 1, target: 2, sender: "写作", recipient: "审稿",
            state: "waiting", round_no: 2, max_rounds: 3, body: "核对第二稿",
        }]);
        const stop = vi.fn();
        mockInvokeHandler("stop_butler_peer_exchange", stop);
        render(<ButlerPeerActivity conversationId={1} />);
        const badge = await screen.findByRole("button", {name: "协作 · 审稿 · 等待答复"});
        expect(screen.queryByText("核对第二稿")).not.toBeInTheDocument();
        fireEvent.click(badge);
        expect(await screen.findByText("核对第二稿")).toBeInTheDocument();
        fireEvent.click(screen.getByRole("button", {name: "停止后续协作"}));
        await waitFor(() => expect(stop).toHaveBeenCalledWith({ conversationId: 1, root: 10 }));
    });

    it("should retry only the failed receipt when the user clicks once or repeatedly", async () => {
        let receiptState = "failed";
        mockInvokeHandler("list_butler_peer_exchanges", () => [failedExchange(receiptState)]);
        let finishRetry!: () => void;
        const retry = vi.fn(() => new Promise<void>(resolve => { finishRetry = resolve; }));
        mockInvokeHandler("retry_butler_peer_receipt", retry);
        render(<ButlerPeerActivity conversationId={1} />);
        const badge = await screen.findByRole("button", {name: "协作 · 审稿 · 失败"});
        fireEvent.click(badge);
        expect(await screen.findByText("回执：投递失败，自动重试已停止")).toBeInTheDocument();
        expect(screen.getByText("仅重发失败或停止通知，不会重新执行原协作。")).toBeInTheDocument();
        expect(retry).not.toHaveBeenCalled();
        // 查看、关闭、重开记录不构成重试授权。
        fireEvent.click(screen.getByRole("button", {name: "Close"}));
        fireEvent.click(badge);
        const button = await screen.findByRole("button", {name: "重试回执"});
        fireEvent.click(button);
        fireEvent.click(button);
        expect(retry).toHaveBeenCalledTimes(1);
        expect(retry).toHaveBeenCalledWith({conversationId: 1, exchangeId: 4});
        expect(screen.getByRole("button", {name: "正在重试回执…"})).toBeDisabled();
        await act(async () => {
            receiptState = "pending";
            await emit("butler_peer_changed");
            finishRetry();
        });
        expect(await screen.findByText("回执：等待投递")).toBeInTheDocument();
        expect(screen.queryByRole("button", {name: /重试回执/})).not.toBeInTheDocument();
        expect(screen.getByText(/写作 → 审稿.*失败/)).toBeInTheDocument();
    });

    it.each([null, "pending", "sending", "delivered"])(
        "should hide receipt retry when receipt state is %s",
        async receiptState => {
            mockInvokeHandler("list_butler_peer_exchanges", () => [failedExchange(receiptState)]);
            render(<ButlerPeerActivity conversationId={1} />);
            fireEvent.click(await screen.findByRole("button", {name: "协作 · 审稿 · 失败"}));
            expect(await screen.findByText("核对第二稿")).toBeInTheDocument();
            expect(screen.queryByRole("button", {name: /重试回执/})).not.toBeInTheDocument();
        }
    );

    it("should allow receipt retry from the recipient when collaboration was stopped", async () => {
        mockInvokeHandler("list_butler_peer_exchanges", () => [{...failedExchange(), state: "stopped", stop_requested: true}]);
        const retry = vi.fn();
        mockInvokeHandler("retry_butler_peer_receipt", retry);
        render(<ButlerPeerActivity conversationId={2} />);
        fireEvent.click(await screen.findByRole("button", {name: "协作 · 写作 · 已停止"}));
        fireEvent.click(await screen.findByRole("button", {name: "重试回执"}));
        await waitFor(() => expect(retry).toHaveBeenCalledWith({conversationId: 2, exchangeId: 4}));
        expect(screen.queryByRole("button", {name: "停止后续协作"})).not.toBeInTheDocument();
    });

    it("should preserve the failure and allow another attempt when retry is rejected", async () => {
        mockInvokeHandler("list_butler_peer_exchanges", () => [failedExchange()]);
        const retry = vi.fn().mockRejectedValueOnce("协作 #4 回执重试失败：数据库忙").mockResolvedValueOnce(undefined);
        mockInvokeHandler("retry_butler_peer_receipt", retry);
        render(<ButlerPeerActivity conversationId={1} />);
        fireEvent.click(await screen.findByRole("button", {name: "协作 · 审稿 · 失败"}));
        fireEvent.click(await screen.findByRole("button", {name: "重试回执"}));
        await waitFor(() => expect(toast.error).toHaveBeenCalledWith("协作 #4 回执重试失败：数据库忙"));
        const button = await screen.findByRole("button", {name: "重试回执"});
        expect(button).toBeEnabled();
        expect(screen.getByText("执行结果不明")).toBeInTheDocument();
        fireEvent.click(button);
        await waitFor(() => expect(retry).toHaveBeenCalledTimes(2));
    });
});
