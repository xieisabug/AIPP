import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { clearAllMockHandlers, mockInvokeHandler } from "@/__tests__/mocks/tauri";
import ButlerPeerActivity from "./ButlerPeerActivity";

afterEach(clearAllMockHandlers);

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
});
